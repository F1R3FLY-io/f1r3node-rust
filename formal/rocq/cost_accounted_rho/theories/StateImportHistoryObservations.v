From Stdlib Require Import List Arith Bool.
From CostAccountedRho Require Import StateImportCodec StateImportCursor StateImportTraversal StateImportWire StateImportOverlay
  StateImportClosure StateImportStorage StateImportCold StateImportEncodedCold.
Import ListNotations.

Definition ImportHistoryByteStore := list nat -> option (list nat).
Definition ImportHistoryObservations := list nat -> option ImportRootLookup.
Definition import_empty_history_observations : ImportHistoryObservations := fun _ => None.

Definition import_history_store_extension (first last : ImportHistoryByteStore) : Prop :=
  forall key bytes, first key = Some bytes -> last key = Some bytes.

Definition import_capture_history_observation (observations : ImportHistoryObservations)
    (key : list nat) (result : ImportRootLookup) : ImportHistoryObservations :=
  fun query => if list_eq_dec Nat.eq_dec query key then
    match observations query with None => Some result | prior => prior end
    else observations query.

Definition import_history_cache_extension (first last : ImportHistoryObservations) : Prop :=
  forall key result, first key = Some result -> last key = Some result.

Theorem import_history_capture_retains_observed_results : forall observations key result,
  import_history_cache_extension observations (import_capture_history_observation observations key result).
Proof.
  intros observations key result query prior Observed. unfold import_capture_history_observation.
  destruct (list_eq_dec Nat.eq_dec query key); now rewrite Observed.
Qed.

Theorem import_history_capture_records_the_first_read : forall observations key result,
  observations key = None -> import_capture_history_observation observations key result key = Some result.
Proof.
  intros observations key result Unread. unfold import_capture_history_observation.
  destruct (list_eq_dec Nat.eq_dec key key); [now rewrite Unread|contradiction].
Qed.

Definition import_project_history_observations (observations : ImportHistoryObservations) : ImportFallibleHistory :=
  fun key => match observations key with Some result => result | None => ImportRootLookupFailure end.

Record ImportHistoryReadRecord : Type := {
  import_history_read_key : list nat;
  import_history_read_store : ImportHistoryByteStore;
  import_history_read_result : ImportRootLookup
}.

Definition import_history_record_valid (record : ImportHistoryReadRecord) : Prop :=
  match import_history_read_result record with
  | ImportRootLookupAbsent => import_history_read_store record (import_history_read_key record) = None
  | ImportRootLookupFound bytes => import_history_read_store record (import_history_read_key record) = Some bytes
  | ImportRootLookupFailure => True
  end.

Definition import_history_successful_lookup (store : ImportHistoryByteStore) (key : list nat) : ImportRootLookup :=
  match store key with Some bytes => ImportRootLookupFound bytes | None => ImportRootLookupAbsent end.

Inductive ImportHistoryAccess : Type :=
| ImportHistoryNeedObservation
| ImportHistoryPreparationFailed
| ImportHistoryAccessResult (result : ImportCheckedHistoryLookup).

Definition import_history_cached_access (Hash : list nat -> list nat) (rows : list ImportReceivedHistoryRow)
    (observations : ImportHistoryObservations) (key : list nat) : ImportHistoryAccess :=
  match observations key with
  | None => ImportHistoryNeedObservation
  | Some _ => match import_prepare_history_overlay Hash (import_project_history_observations observations) rows with
    | ImportHistoryOverlayReady candidate =>
        ImportHistoryAccessResult (import_check_overlay_history_lookup Hash candidate key)
    | _ => ImportHistoryPreparationFailed
    end
  end.

Record ImportHistoryLogicalRead : Type := {
  import_history_logical_key : list nat;
  import_history_logical_cache : ImportHistoryObservations;
  import_history_logical_result : ImportHistoryAccess
}.

Inductive import_history_observation_trace (Hash : list nat -> list nat)
    (rows : list ImportReceivedHistoryRow) (initial : ImportHistoryByteStore) :
    list ImportHistoryReadRecord -> list ImportHistoryLogicalRead ->
    ImportHistoryByteStore -> ImportHistoryObservations -> Prop :=
| import_history_observation_begin :
    import_history_observation_trace Hash rows initial [] [] initial import_empty_history_observations
| import_history_observation_success : forall records logical store observations key,
    import_history_observation_trace Hash rows initial records logical store observations -> observations key = None ->
    import_history_observation_trace Hash rows initial
      (records ++ [{| import_history_read_key := key; import_history_read_store := store;
        import_history_read_result := import_history_successful_lookup store key |}]) logical store
      (import_capture_history_observation observations key (import_history_successful_lookup store key))
| import_history_observation_failure : forall records logical store observations key,
    import_history_observation_trace Hash rows initial records logical store observations -> observations key = None ->
    import_history_observation_trace Hash rows initial
      (records ++ [{| import_history_read_key := key; import_history_read_store := store;
        import_history_read_result := ImportRootLookupFailure |}]) logical store
      (import_capture_history_observation observations key ImportRootLookupFailure)
| import_history_observation_write : forall records logical store observations next,
    import_history_observation_trace Hash rows initial records logical store observations ->
    import_history_store_extension store next ->
    import_history_observation_trace Hash rows initial records logical next observations
| import_history_logical_lookup : forall records logical store observations key,
    import_history_observation_trace Hash rows initial records logical store observations ->
    import_history_observation_trace Hash rows initial records
      (logical ++ [{| import_history_logical_key := key; import_history_logical_cache := observations;
        import_history_logical_result := import_history_cached_access Hash rows observations key |}])
      store observations.

Definition import_history_records_retained (records : list ImportHistoryReadRecord)
    (observations : ImportHistoryObservations) : Prop :=
  Forall (fun record => observations (import_history_read_key record) = Some (import_history_read_result record)) records.

Theorem import_history_capture_preserves_earlier_records : forall records observations key result,
  import_history_records_retained records observations ->
  import_history_records_retained records (import_capture_history_observation observations key result).
Proof.
  intros records observations key result Retained. unfold import_history_records_retained in *.
  eapply Forall_impl; [|exact Retained]. intros record Recorded.
  now apply import_history_capture_retains_observed_results.
Qed.

Theorem import_history_trace_retains_every_physical_result : forall Hash rows initial records logical current observations,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_history_records_retained records observations.
Proof.
  intros Hash rows initial records logical current observations Trace. induction Trace.
  - constructor.
  - apply Forall_app. split; [now apply import_history_capture_preserves_earlier_records|].
    constructor; [|constructor]. cbn [import_history_read_key import_history_read_result].
    now apply import_history_capture_records_the_first_read.
  - apply Forall_app. split; [now apply import_history_capture_preserves_earlier_records|].
    constructor; [|constructor]. cbn [import_history_read_key import_history_read_result].
    now apply import_history_capture_records_the_first_read.
  - exact IHTrace.
  - exact IHTrace.
Qed.

Definition import_history_observations_have_origins (records : list ImportHistoryReadRecord)
    (observations : ImportHistoryObservations) : Prop :=
  forall key result, observations key = Some result -> exists record,
    In record records /\ import_history_read_key record = key /\ import_history_read_result record = result.

Theorem import_history_capture_extends_origins : forall records observations key result store,
  import_history_observations_have_origins records observations -> observations key = None ->
  import_history_observations_have_origins
    (records ++ [{| import_history_read_key := key; import_history_read_store := store; import_history_read_result := result |}])
    (import_capture_history_observation observations key result).
Proof.
  intros records observations key result store Origins Unread query answer Captured.
  unfold import_capture_history_observation in Captured.
  destruct (list_eq_dec Nat.eq_dec query key) as [Same|Different].
  - subst query. rewrite Unread in Captured. inversion Captured; subst answer.
    exists {| import_history_read_key := key; import_history_read_store := store; import_history_read_result := result |}.
    split; [apply in_or_app; right; now left|auto].
  - destruct (Origins _ _ Captured) as [record [Present Rest]].
    exists record. split; [apply in_or_app; now left|exact Rest].
Qed.

Theorem import_history_trace_derives_observation_origins : forall Hash rows initial records logical current observations,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_history_observations_have_origins records observations.
Proof.
  intros Hash rows initial records logical current observations Trace. induction Trace.
  - intros key result Impossible. discriminate.
  - eapply import_history_capture_extends_origins; eauto.
  - eapply import_history_capture_extends_origins; eauto.
  - exact IHTrace.
  - exact IHTrace.
Qed.

Theorem import_history_trace_records_exact_read_states : forall Hash rows initial records logical current observations,
  import_history_observation_trace Hash rows initial records logical current observations ->
  Forall import_history_record_valid records.
Proof.
  intros Hash rows initial records logical current observations Trace. induction Trace.
  - constructor.
  - apply Forall_app. split; [exact IHTrace|]. constructor; [|constructor].
    cbn [import_history_record_valid import_history_read_result import_history_read_store import_history_read_key].
    unfold import_history_successful_lookup. destruct (store key) eqn:Read; cbn; exact Read.
  - apply Forall_app. split; [exact IHTrace|]. constructor; [exact I|constructor].
  - exact IHTrace.
  - exact IHTrace.
Qed.

Theorem import_unread_history_key_has_no_physical_record : forall records observations key,
  import_history_records_retained records observations -> observations key = None ->
  ~ In key (map import_history_read_key records).
Proof.
  intros records observations key Retained Unread Present.
  apply in_map_iff in Present as [record [Key Present]].
  unfold import_history_records_retained in Retained. rewrite Forall_forall in Retained.
  specialize (Retained _ Present). rewrite Key, Unread in Retained. discriminate.
Qed.

Theorem import_history_trace_reads_each_key_once : forall Hash rows initial records logical current observations,
  import_history_observation_trace Hash rows initial records logical current observations ->
  NoDup (map import_history_read_key records).
Proof.
  intros Hash rows initial records logical current observations Trace. induction Trace.
  - constructor.
  - rewrite map_app. apply NoDup_app; [exact IHTrace| |].
    + constructor; [simpl; tauto|constructor].
    + intros key' Earlier New. simpl in New. destruct New as [Same|Impossible]; [|contradiction]. subst key'.
      eapply import_unread_history_key_has_no_physical_record; eauto.
      eapply import_history_trace_retains_every_physical_result; eauto.
  - rewrite map_app. apply NoDup_app; [exact IHTrace| |].
    + constructor; [simpl; tauto|constructor].
    + intros key' Earlier New. simpl in New. destruct New as [Same|Impossible]; [|contradiction]. subst key'.
      eapply import_unread_history_key_has_no_physical_record; eauto.
      eapply import_history_trace_retains_every_physical_result; eauto.
  - exact IHTrace.
  - exact IHTrace.
Qed.

Definition import_history_logical_records_valid (Hash : list nat -> list nat)
    (rows : list ImportReceivedHistoryRow) (logical : list ImportHistoryLogicalRead)
    (observations : ImportHistoryObservations) : Prop :=
  Forall (fun record =>
    import_history_logical_result record =
      import_history_cached_access Hash rows (import_history_logical_cache record) (import_history_logical_key record) /\
    import_history_cache_extension (import_history_logical_cache record) observations) logical.

Theorem import_history_trace_records_each_logical_lookup : forall Hash rows initial records logical current observations,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_history_logical_records_valid Hash rows logical observations.
Proof.
  intros Hash rows initial records logical current observations Trace. induction Trace.
  - constructor.
  - eapply Forall_impl; [|exact IHTrace]. intros record [Computed Retained]. split; [exact Computed|].
    intros key' result Old. apply import_history_capture_retains_observed_results. now apply Retained.
  - eapply Forall_impl; [|exact IHTrace]. intros record [Computed Retained]. split; [exact Computed|].
    intros key' result Old. apply import_history_capture_retains_observed_results. now apply Retained.
  - exact IHTrace.
  - apply Forall_app. split; [exact IHTrace|]. constructor; [|constructor]. split; [reflexivity|].
    intros query result Observed. exact Observed.
Qed.

Theorem import_prepared_history_rows_have_successful_compatibility_observations :
  forall Hash observations rows candidate key bytes,
  import_prepare_history_overlay Hash (import_project_history_observations observations) rows =
    ImportHistoryOverlayReady candidate -> In (key, bytes) rows ->
  observations key = Some ImportRootLookupAbsent \/ observations key = Some (ImportRootLookupFound bytes).
Proof.
  intros Hash observations rows candidate key bytes Prepared Present.
  destruct (import_prepared_history_rows_are_initially_compatible_and_consistent _ _ _ _ Prepared) as [Compatible _].
  unfold import_history_rows_compatible in Compatible. rewrite Forall_forall in Compatible.
  destruct (Compatible _ Present) as [_ Binding]. cbn [fst snd] in Binding.
  unfold import_project_history_observations in Binding.
  destruct (observations key) as [result|] eqn:Observed; [destruct result|]; intuition congruence.
Qed.

Theorem import_history_overlay_never_hides_unread_or_failed_received_keys :
  forall Hash observations rows candidate key bytes,
  In (key, bytes) rows ->
  (observations key = None \/ observations key = Some ImportRootLookupFailure) ->
  import_prepare_history_overlay Hash (import_project_history_observations observations) rows <>
    ImportHistoryOverlayReady candidate.
Proof.
  intros Hash observations rows candidate key bytes Present Failed Prepared.
  destruct (import_prepared_history_rows_have_successful_compatibility_observations
    _ _ _ _ _ _ Prepared Present); intuition congruence.
Qed.

Theorem import_checked_history_access_has_observed_byte_origin : forall Hash rows observations key edges,
  import_history_cached_access Hash rows observations key = ImportHistoryAccessResult (ImportCheckedHistoryFound edges) ->
  exists bytes,
    length key = 32 /\ Forall import_wire_byte key /\ Hash bytes = key /\ import_parse_wire_node bytes = Some edges /\
    ((In (key, bytes) rows /\
      (observations key = Some ImportRootLookupAbsent \/ observations key = Some (ImportRootLookupFound bytes))) \/
     (~ In key (map fst rows) /\ observations key = Some (ImportRootLookupFound bytes))).
Proof.
  intros Hash rows observations key edges Access. unfold import_history_cached_access in Access.
  destruct (observations key) as [observed|] eqn:Observed; [|discriminate].
  destruct (import_prepare_history_overlay Hash (import_project_history_observations observations) rows)
    as [candidate|invalid|conflict|failure] eqn:Prepared; try discriminate.
  assert (import_overlay_traversal_reader Hash candidate key = Some edges) as Read.
  { unfold import_overlay_traversal_reader. inversion Access. now rewrite H0. }
  destruct (import_overlay_traversal_has_checked_byte_evidence _ _ _ _ Read)
    as [bytes [Found [Width [Domain [Hashed Parsed]]]]].
  exists bytes. split; [exact Width|]. split; [exact Domain|]. split; [exact Hashed|]. split; [exact Parsed|].
  destruct (in_dec (list_eq_dec Nat.eq_dec) key (map fst rows)) as [Received|Fallback].
  - apply in_map_iff in Received as [[row_key row_bytes] [Key Present]]. cbn [fst] in Key. subst row_key.
    pose proof (import_history_overlay_authenticates_every_received_row _ _ _ _ Prepared) as ValidRows.
    rewrite Forall_forall in ValidRows. destruct (ValidRows _ Present) as [_ Stored]. cbn [fst snd] in Stored.
    rewrite Found in Stored. inversion Stored; subst row_bytes. left. split; [exact Present|].
    rewrite <- Observed.
    eapply import_prepared_history_rows_have_successful_compatibility_observations; eauto.
  - right. split; [exact Fallback|].
    rewrite (import_history_overlay_does_not_change_unreceived_keys _ _ _ _ _ Prepared Fallback) in Found.
    unfold import_project_history_observations in Found. rewrite Observed in Found. inversion Found. now subst observed.
Qed.

Theorem import_retained_history_observations_preserve_prepared_overlays :
  forall Hash rows first last candidate,
  import_history_cache_extension first last ->
  import_prepare_history_overlay Hash (import_project_history_observations first) rows =
    ImportHistoryOverlayReady candidate ->
  exists result,
    import_prepare_history_overlay Hash (import_project_history_observations last) rows =
      ImportHistoryOverlayReady result /\ import_history_observation_extension candidate result.
Proof.
  intros Hash rows first last candidate Retained Prepared.
  destruct (import_prepared_history_rows_are_initially_compatible_and_consistent _ _ _ _ Prepared)
    as [Compatible Consistent].
  assert (import_history_rows_compatible Hash (import_project_history_observations last) rows) as Later.
  { unfold import_history_rows_compatible in *. rewrite Forall_forall in *.
    intros [key bytes] Present. destruct (Compatible _ Present) as [Checked _].
    split; [exact Checked|]. cbn [fst snd].
    destruct (import_prepared_history_rows_have_successful_compatibility_observations
      _ _ _ _ _ _ Prepared Present) as [Absent|Found]; unfold import_project_history_observations.
    - rewrite (Retained _ _ Absent). now left.
    - rewrite (Retained _ _ Found). now right. }
  destruct (import_valid_compatible_consistent_history_rows_can_be_prepared _ _ _ Later Consistent)
    as [result Result]. exists result. split; [exact Result|].
  intros key bytes Read.
  destruct (in_dec (list_eq_dec Nat.eq_dec) key (map fst rows)) as [Received|Fallback].
  - apply in_map_iff in Received as [[row_key row_bytes] [Key Present]]. cbn [fst] in Key. subst row_key.
    pose proof (import_history_overlay_authenticates_every_received_row _ _ _ _ Prepared) as FirstRows.
    pose proof (import_history_overlay_authenticates_every_received_row _ _ _ _ Result) as LastRows.
    rewrite Forall_forall in FirstRows, LastRows.
    destruct (FirstRows _ Present) as [_ FirstRead]. destruct (LastRows _ Present) as [_ LastRead].
    cbn [fst snd] in *. congruence.
  - rewrite (import_history_overlay_does_not_change_unreceived_keys _ _ _ _ _ Prepared Fallback) in Read.
    rewrite (import_history_overlay_does_not_change_unreceived_keys _ _ _ _ _ Result Fallback).
    unfold import_project_history_observations in *.
    destruct (first key) as [answer|] eqn:Observed; [|discriminate].
    inversion Read; subst answer. now rewrite (Retained _ _ Observed).
Qed.

Theorem import_history_cache_growth_preserves_successful_logical_reads :
  forall Hash rows first last key edges,
  import_history_cache_extension first last ->
  import_history_cached_access Hash rows first key = ImportHistoryAccessResult (ImportCheckedHistoryFound edges) ->
  import_history_cached_access Hash rows last key = ImportHistoryAccessResult (ImportCheckedHistoryFound edges).
Proof.
  intros Hash rows first last key edges Retained Read.
  unfold import_history_cached_access in Read.
  destruct (first key) as [answer|] eqn:Observed; [|discriminate].
  destruct (import_prepare_history_overlay Hash (import_project_history_observations first) rows)
    as [candidate|invalid|conflict|failed] eqn:Prepared; try discriminate.
  destruct (import_retained_history_observations_preserve_prepared_overlays _ _ _ _ _ Retained Prepared)
    as [result [Result Extension]].
  unfold import_history_cached_access. rewrite (Retained _ _ Observed), Result.
  inversion Read as [Checked].
  assert (import_overlay_traversal_reader Hash result key = Some edges) as Final.
  { eapply import_checked_overlay_reads_survive_compatible_observations; [exact Extension|].
    unfold import_overlay_traversal_reader. now rewrite Checked. }
  unfold import_overlay_traversal_reader in Final.
  destruct (import_check_overlay_history_lookup Hash result key); congruence.
Qed.

Theorem import_history_logical_trace_has_exact_successful_final_view :
  forall Hash rows initial records logical current observations event edges,
  import_history_observation_trace Hash rows initial records logical current observations -> In event logical ->
  import_history_logical_result event = ImportHistoryAccessResult (ImportCheckedHistoryFound edges) ->
  import_history_cached_access Hash rows observations (import_history_logical_key event) =
    ImportHistoryAccessResult (ImportCheckedHistoryFound edges).
Proof.
  intros Hash rows initial records logical current observations event edges Trace Present Success.
  pose proof (import_history_trace_records_each_logical_lookup _ _ _ _ _ _ _ Trace) as Logical.
  unfold import_history_logical_records_valid in Logical. rewrite Forall_forall in Logical.
  destruct (Logical _ Present) as [Computed Retained]. rewrite Computed in Success.
  eapply import_history_cache_growth_preserves_successful_logical_reads; eauto.
Qed.

Theorem import_history_trace_exposes_only_physically_observed_checked_bytes :
  forall Hash rows initial records logical current observations event edges,
  import_history_observation_trace Hash rows initial records logical current observations -> In event logical ->
  import_history_logical_result event = ImportHistoryAccessResult (ImportCheckedHistoryFound edges) ->
  exists bytes record,
    In record records /\ import_history_read_key record = import_history_logical_key event /\
    import_history_record_valid record /\
    length (import_history_logical_key event) = 32 /\ Forall import_wire_byte (import_history_logical_key event) /\
    Hash bytes = import_history_logical_key event /\ import_parse_wire_node bytes = Some edges /\
    ((In (import_history_logical_key event, bytes) rows /\
      (import_history_read_result record = ImportRootLookupAbsent \/
       import_history_read_result record = ImportRootLookupFound bytes)) \/
     (~ In (import_history_logical_key event) (map fst rows) /\
       import_history_read_result record = ImportRootLookupFound bytes)).
Proof.
  intros Hash rows initial records logical current observations event edges Trace Present Success.
  pose proof (import_history_trace_records_each_logical_lookup _ _ _ _ _ _ _ Trace) as Logical.
  unfold import_history_logical_records_valid in Logical. rewrite Forall_forall in Logical.
  destruct (Logical _ Present) as [Computed Retained]. rewrite Computed in Success.
  destruct (import_checked_history_access_has_observed_byte_origin _ _ _ _ _ Success)
    as [bytes [Width [Domain [Hashed [Parsed Origin]]]]].
  assert (exists answer, observations (import_history_logical_key event) = Some answer /\
    ((In (import_history_logical_key event, bytes) rows /\
      (answer = ImportRootLookupAbsent \/ answer = ImportRootLookupFound bytes)) \/
     (~ In (import_history_logical_key event) (map fst rows) /\ answer = ImportRootLookupFound bytes))) as Final.
  { destruct Origin as [[Received [Absent|Found]]|[Fallback Found]].
    - exists ImportRootLookupAbsent. split; [now apply Retained|auto].
    - exists (ImportRootLookupFound bytes). split; [now apply Retained|auto].
    - exists (ImportRootLookupFound bytes). split; [now apply Retained|auto]. }
  destruct Final as [answer [Observed FinalOrigin]].
  destruct (import_history_trace_derives_observation_origins _ _ _ _ _ _ _ Trace _ _ Observed)
    as [record [Recorded [Key Result]]].
  pose proof (import_history_trace_records_exact_read_states _ _ _ _ _ _ _ Trace) as Valid.
  rewrite Forall_forall in Valid. specialize (Valid _ Recorded).
  exists bytes, record. split; [exact Recorded|]. split; [exact Key|]. split; [exact Valid|].
  split; [exact Width|]. split; [exact Domain|]. split; [exact Hashed|]. split; [exact Parsed|].
  now rewrite Result.
Qed.

Theorem import_unread_history_lookup_requests_an_observation : forall Hash rows observations key,
  observations key = None -> import_history_cached_access Hash rows observations key = ImportHistoryNeedObservation.
Proof. intros. unfold import_history_cached_access. now rewrite H. Qed.

Theorem import_history_same_attempt_refresh_cannot_replace_failure_or_absence :
  forall observations key bytes prior,
  (prior = ImportRootLookupFailure \/ prior = ImportRootLookupAbsent) -> observations key = Some prior ->
  import_capture_history_observation observations key (ImportRootLookupFound bytes) key = Some prior.
Proof. intros. now apply import_history_capture_retains_observed_results. Qed.

Example import_unread_received_root_cannot_prepare_by_default :
  let key := repeat 0 32 in
  import_prepare_history_overlay (fun _ => key)
    (import_project_history_observations import_empty_history_observations) [(key, [])] =
    ImportHistoryOverlayStorageFailure key.
Proof. vm_compute. reflexivity. Qed.

Example import_unobserved_ancestor_cannot_supply_edges :
  import_history_cached_access (fun _ => repeat 0 32) [] import_empty_history_observations (repeat 0 32) =
    ImportHistoryNeedObservation.
Proof. reflexivity. Qed.

Definition import_history_record_bytes_retained (current : ImportHistoryByteStore)
    (record : ImportHistoryReadRecord) : Prop :=
  forall bytes, import_history_read_result record = ImportRootLookupFound bytes ->
    current (import_history_read_key record) = Some bytes.

Theorem import_history_trace_preserves_all_successfully_observed_bytes :
  forall Hash rows initial records logical current observations,
  import_history_observation_trace Hash rows initial records logical current observations ->
  Forall (import_history_record_bytes_retained current) records.
Proof.
  intros Hash rows initial records logical current observations Trace. induction Trace.
  - constructor.
  - apply Forall_app. split; [exact IHTrace|]. constructor; [|constructor].
    intros bytes Read. cbn [import_history_read_result import_history_read_key] in *.
    unfold import_history_successful_lookup in Read. destruct (store key); congruence.
  - apply Forall_app. split; [exact IHTrace|]. constructor; [|constructor].
    intros bytes Impossible. discriminate.
  - eapply Forall_impl; [|exact IHTrace]. intros record Retained bytes Read.
    apply H. now apply Retained.
  - exact IHTrace.
Qed.

Theorem import_history_checked_bytes_are_current_or_await_received_absence_comparison :
  forall Hash rows initial records logical current observations event edges,
  import_history_observation_trace Hash rows initial records logical current observations -> In event logical ->
  import_history_logical_result event = ImportHistoryAccessResult (ImportCheckedHistoryFound edges) ->
  exists bytes,
    Hash bytes = import_history_logical_key event /\ import_parse_wire_node bytes = Some edges /\
    (current (import_history_logical_key event) = Some bytes \/
      (In (import_history_logical_key event, bytes) rows /\ exists record,
        In record records /\ import_history_read_key record = import_history_logical_key event /\
        import_history_read_result record = ImportRootLookupAbsent /\
        import_history_read_store record (import_history_logical_key event) = None)).
Proof.
  intros Hash rows initial records logical current observations event edges Trace Present Success.
  destruct (import_history_trace_exposes_only_physically_observed_checked_bytes
    _ _ _ _ _ _ _ _ _ Trace Present Success)
    as [bytes [record [Recorded [Key [Valid [Width [Domain [Hashed [Parsed Origin]]]]]]]]].
  pose proof (import_history_trace_preserves_all_successfully_observed_bytes _ _ _ _ _ _ _ Trace) as Retained.
  rewrite Forall_forall in Retained. specialize (Retained _ Recorded).
  exists bytes. split; [exact Hashed|]. split; [exact Parsed|].
  destruct Origin as [[Received [Absent|Found]]|[Fallback Found]].
  - right. split; [exact Received|]. exists record.
    split; [exact Recorded|]. split; [exact Key|]. split; [exact Absent|].
    unfold import_history_record_valid in Valid. now rewrite Absent, Key in Valid.
  - left. rewrite <- Key. now apply Retained.
  - left. rewrite <- Key. now apply Retained.
Qed.

Definition import_materialize_history_overlay (candidate : ImportFallibleHistory) : ImportHistoryByteStore :=
  fun key => match candidate key with ImportRootLookupFound bytes => Some bytes | _ => None end.

Definition import_observed_history_batch (Hash : list nat -> list nat)
    (observations : ImportHistoryObservations) (current : ImportHistoryByteStore)
    (rows : list ImportReceivedHistoryRow) (storage_ok : bool) : ImportHistoryByteStore * bool :=
  match import_prepare_history_overlay Hash (import_project_history_observations observations) rows with
  | ImportHistoryOverlayReady _ =>
      match import_prepare_history_overlay Hash (import_history_successful_lookup current) rows with
      | ImportHistoryOverlayReady candidate =>
          if storage_ok then (import_materialize_history_overlay candidate, true) else (current, false)
      | _ => (current, false)
      end
  | _ => (current, false)
  end.

Definition import_history_transaction_row (before : ImportFallibleHistory)
    (row : ImportReceivedHistoryRow) : ImportHistoryOverlayResult :=
  let '(key, bytes) := row in
  match before key with
  | ImportRootLookupAbsent => ImportHistoryOverlayReady (import_candidate_history_insert before key bytes)
  | ImportRootLookupFailure => ImportHistoryOverlayStorageFailure key
  | ImportRootLookupFound existing =>
      if list_eq_dec Nat.eq_dec existing bytes then ImportHistoryOverlayReady before
      else ImportHistoryOverlayConflict key
  end.

Fixpoint import_history_transaction_rows (before : ImportFallibleHistory)
    (rows : list ImportReceivedHistoryRow) : ImportHistoryOverlayResult :=
  match rows with
  | [] => ImportHistoryOverlayReady before
  | row :: rest => match import_history_transaction_row before row with
    | ImportHistoryOverlayReady candidate => import_history_transaction_rows candidate rest
    | failure => failure
    end
  end.

Theorem import_prevalidated_history_row_uses_only_the_current_byte_guard :
  forall Hash before key bytes edges,
  import_check_received_history_row Hash key bytes = Some edges ->
  import_stage_history_row Hash before (key, bytes) = import_history_transaction_row before (key, bytes).
Proof.
  intros. unfold import_stage_history_row, import_history_transaction_row. now rewrite H.
Qed.

Theorem import_prevalidated_history_batch_uses_only_current_byte_guards :
  forall Hash rows before,
  Forall (fun row => exists edges, import_check_received_history_row Hash (fst row) (snd row) = Some edges) rows ->
  import_prepare_history_overlay Hash before rows = import_history_transaction_rows before rows.
Proof.
  intros Hash rows. induction rows as [|[key bytes] rest IH]; intros before Valid; [reflexivity|].
  inversion Valid as [|row tail [edges Checked] Remaining]; subst row tail.
  cbn [fst snd] in Checked. cbn [import_prepare_history_overlay import_history_transaction_rows].
  rewrite (import_prevalidated_history_row_uses_only_the_current_byte_guard _ _ _ _ _ Checked).
  destruct (import_history_transaction_row before (key, bytes)); auto.
Qed.

Theorem import_history_batch_commit_can_reuse_preflight_authentication :
  forall Hash observations current rows observed storage_ok,
  import_prepare_history_overlay Hash (import_project_history_observations observations) rows =
    ImportHistoryOverlayReady observed ->
  import_observed_history_batch Hash observations current rows storage_ok =
    match import_history_transaction_rows (import_history_successful_lookup current) rows with
    | ImportHistoryOverlayReady candidate =>
        if storage_ok then (import_materialize_history_overlay candidate, true) else (current, false)
    | _ => (current, false)
    end.
Proof.
  intros Hash observations current rows observed storage_ok Prepared.
  unfold import_observed_history_batch. rewrite Prepared.
  rewrite import_prevalidated_history_batch_uses_only_current_byte_guards; [reflexivity|].
  apply import_history_overlay_authenticates_every_received_row in Prepared.
  eapply Forall_impl; [|exact Prepared]. intros row [Checked _]. exact Checked.
Qed.

Theorem import_successful_history_batch_has_both_checked_stages :
  forall Hash observations current rows storage_ok result,
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  storage_ok = true /\ exists observed candidate,
    import_prepare_history_overlay Hash (import_project_history_observations observations) rows =
      ImportHistoryOverlayReady observed /\
    import_prepare_history_overlay Hash (import_history_successful_lookup current) rows =
      ImportHistoryOverlayReady candidate /\ result = import_materialize_history_overlay candidate.
Proof.
  intros Hash observations current rows storage_ok result Commit.
  unfold import_observed_history_batch in Commit.
  destruct (import_prepare_history_overlay Hash (import_project_history_observations observations) rows)
    as [observed|invalid|conflict|failed] eqn:Observed; try discriminate.
  destruct (import_prepare_history_overlay Hash (import_history_successful_lookup current) rows)
    as [candidate|invalid|conflict|failed] eqn:Current; try discriminate.
  destruct storage_ok; inversion Commit; subst. split; [reflexivity|].
  exists observed, candidate. auto.
Qed.

Theorem import_failed_history_batch_preserves_the_entire_commit_store :
  forall Hash observations current rows storage_ok result,
  import_observed_history_batch Hash observations current rows storage_ok = (result, false) -> result = current.
Proof.
  intros Hash observations current rows storage_ok result Commit.
  unfold import_observed_history_batch in Commit.
  destruct (import_prepare_history_overlay Hash (import_project_history_observations observations) rows);
    try (inversion Commit; reflexivity).
  destruct (import_prepare_history_overlay Hash (import_history_successful_lookup current) rows);
    try (inversion Commit; reflexivity).
  destruct storage_ok; inversion Commit; reflexivity.
Qed.

Theorem import_history_batch_preserves_all_existing_bytes :
  forall Hash observations current rows storage_ok result success,
  import_observed_history_batch Hash observations current rows storage_ok = (result, success) ->
  import_history_store_extension current result.
Proof.
  intros Hash observations current rows storage_ok result success Commit key bytes Stored.
  destruct success.
  - destruct (import_successful_history_batch_has_both_checked_stages _ _ _ _ _ _ Commit)
      as [_ [observed [candidate [_ [Prepared Same]]]]]. subst result.
    unfold import_materialize_history_overlay.
    assert (candidate key = ImportRootLookupFound bytes) as Kept.
    { eapply import_history_overlay_preserves_every_observed_binding; [exact Prepared|].
      unfold import_history_successful_lookup. now rewrite Stored. }
    now rewrite Kept.
  - apply import_failed_history_batch_preserves_the_entire_commit_store in Commit.
    now subst result.
Qed.

Theorem import_history_batch_writes_only_authenticated_received_bytes :
  forall Hash observations current rows storage_ok result,
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  Forall (fun row =>
    (exists edges, import_check_received_history_row Hash (fst row) (snd row) = Some edges) /\
    result (fst row) = Some (snd row)) rows.
Proof.
  intros Hash observations current rows storage_ok result Commit.
  destruct (import_successful_history_batch_has_both_checked_stages _ _ _ _ _ _ Commit)
    as [_ [observed [candidate [_ [Prepared Same]]]]]. subst result.
  apply import_history_overlay_authenticates_every_received_row in Prepared.
  eapply Forall_impl; [|exact Prepared]. intros [key bytes] [Checked Found].
  cbn [fst snd] in *. split; [exact Checked|]. unfold import_materialize_history_overlay. now rewrite Found.
Qed.

Theorem import_history_batch_preserves_every_unreceived_key :
  forall Hash observations current rows storage_ok result success key,
  import_observed_history_batch Hash observations current rows storage_ok = (result, success) ->
  ~ In key (map fst rows) -> result key = current key.
Proof.
  intros Hash observations current rows storage_ok result success key Commit Unreceived.
  destruct success.
  - destruct (import_successful_history_batch_has_both_checked_stages _ _ _ _ _ _ Commit)
      as [_ [observed [candidate [_ [Prepared Same]]]]]. subst result.
    unfold import_materialize_history_overlay.
    rewrite (import_history_overlay_does_not_change_unreceived_keys _ _ _ _ _ Prepared Unreceived).
    unfold import_history_successful_lookup. now destruct (current key).
  - apply import_failed_history_batch_preserves_the_entire_commit_store in Commit. now subst result.
Qed.

Theorem import_history_batch_rechecks_stale_absence_without_overwriting :
  forall Hash observations current rows storage_ok key received existing,
  In (key, received) rows -> current key = Some existing -> existing <> received ->
  import_observed_history_batch Hash observations current rows storage_ok = (current, false).
Proof.
  intros Hash observations current rows storage_ok key received existing Present Stored Different.
  destruct (import_observed_history_batch Hash observations current rows storage_ok)
    as [result success] eqn:Commit. destruct success.
  - pose proof (import_history_batch_preserves_all_existing_bytes _ _ _ _ _ _ _ Commit key existing Stored) as Kept.
    pose proof (import_history_batch_writes_only_authenticated_received_bytes _ _ _ _ _ _ Commit) as Rows.
    rewrite Forall_forall in Rows. destruct (Rows (key, received) Present) as [_ Written].
    cbn [fst snd] in Written. congruence.
  - apply import_failed_history_batch_preserves_the_entire_commit_store in Commit. now subst result.
Qed.

Theorem import_history_batch_requires_successful_prior_observations :
  forall Hash observations current rows storage_ok result key bytes,
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  In (key, bytes) rows ->
  observations key = Some ImportRootLookupAbsent \/ observations key = Some (ImportRootLookupFound bytes).
Proof.
  intros Hash observations current rows storage_ok result key bytes Commit Present.
  destruct (import_successful_history_batch_has_both_checked_stages _ _ _ _ _ _ Commit)
    as [_ [observed [candidate [Prepared _]]]].
  eapply import_prepared_history_rows_have_successful_compatibility_observations; eauto.
Qed.

Theorem import_history_batch_rejects_unread_or_failed_received_keys :
  forall Hash observations current rows storage_ok key bytes,
  In (key, bytes) rows ->
  (observations key = None \/ observations key = Some ImportRootLookupFailure) ->
  import_observed_history_batch Hash observations current rows storage_ok = (current, false).
Proof.
  intros Hash observations current rows storage_ok key bytes Present Failed.
  destruct (import_observed_history_batch Hash observations current rows storage_ok)
    as [result success] eqn:Commit. destruct success.
  - destruct (import_history_batch_requires_successful_prior_observations _ _ _ _ _ _ _ _ Commit Present);
      intuition congruence.
  - apply import_failed_history_batch_preserves_the_entire_commit_store in Commit. now subst result.
Qed.

Theorem import_history_batch_rejects_conflicting_duplicates_atomically :
  forall Hash observations current rows storage_ok key first second,
  In (key, first) rows -> In (key, second) rows -> first <> second ->
  import_observed_history_batch Hash observations current rows storage_ok = (current, false).
Proof.
  intros Hash observations current rows storage_ok key first second First Second Different.
  unfold import_observed_history_batch.
  destruct (import_prepare_history_overlay Hash (import_project_history_observations observations) rows)
    as [candidate|invalid|conflict|failed] eqn:Prepared; try reflexivity.
  exfalso. exact (import_history_overlay_rejects_conflicting_duplicate_rows Hash
    (import_project_history_observations observations) rows key first second
    First Second Different candidate Prepared).
Qed.

Theorem import_history_batch_storage_failure_discards_every_staged_row :
  forall Hash observations current rows,
  import_observed_history_batch Hash observations current rows false = (current, false).
Proof.
  intros. unfold import_observed_history_batch.
  destruct (import_prepare_history_overlay Hash (import_project_history_observations observations) rows);
    try reflexivity.
  destruct (import_prepare_history_overlay Hash (import_history_successful_lookup current) rows); reflexivity.
Qed.

Theorem import_valid_history_batch_accepts_compatible_current_bytes :
  forall Hash observations current rows observed,
  import_prepare_history_overlay Hash (import_project_history_observations observations) rows =
    ImportHistoryOverlayReady observed ->
  (forall key bytes, In (key, bytes) rows -> current key = None \/ current key = Some bytes) ->
  exists result, import_observed_history_batch Hash observations current rows true = (result, true).
Proof.
  intros Hash observations current rows observed Prepared Compatible.
  destruct (import_prepared_history_rows_are_initially_compatible_and_consistent _ _ _ _ Prepared)
    as [Checked Consistent].
  assert (import_history_rows_compatible Hash (import_history_successful_lookup current) rows) as Current.
  { unfold import_history_rows_compatible in *. rewrite Forall_forall in *.
    intros [key bytes] Present. destruct (Checked (key, bytes) Present) as [Valid _].
    split; [exact Valid|]. cbn [fst snd]. unfold import_history_successful_lookup.
    destruct (Compatible key bytes Present) as [Absent|Found]; rewrite ?Absent, ?Found; auto. }
  destruct (import_valid_compatible_consistent_history_rows_can_be_prepared _ _ _ Current Consistent)
    as [candidate Staged].
  exists (import_materialize_history_overlay candidate). unfold import_observed_history_batch.
  now rewrite Prepared, Staged.
Qed.

Theorem import_history_batch_has_actual_successful_read_witnesses :
  forall Hash rows initial records logical current observations storage_ok result key bytes,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  In (key, bytes) rows ->
  exists record, In record records /\ import_history_read_key record = key /\
    ((import_history_read_result record = ImportRootLookupAbsent /\ import_history_read_store record key = None) \/
     (import_history_read_result record = ImportRootLookupFound bytes /\ import_history_read_store record key = Some bytes)).
Proof.
  intros Hash rows initial records logical current observations storage_ok result key bytes Trace Commit Present.
  pose proof (import_history_trace_derives_observation_origins _ _ _ _ _ _ _ Trace) as Origins.
  pose proof (import_history_trace_records_exact_read_states _ _ _ _ _ _ _ Trace) as Valid.
  rewrite Forall_forall in Valid.
  destruct (import_history_batch_requires_successful_prior_observations _ _ _ _ _ _ _ _ Commit Present)
    as [Absent|Found].
  - destruct (Origins _ _ Absent) as [record [Recorded [Key Read]]].
    exists record. split; [exact Recorded|]. split; [exact Key|]. left. split; [exact Read|].
    specialize (Valid _ Recorded). unfold import_history_record_valid in Valid. now rewrite Read, Key in Valid.
  - destruct (Origins _ _ Found) as [record [Recorded [Key Read]]].
    exists record. split; [exact Recorded|]. split; [exact Key|]. right. split; [exact Read|].
    specialize (Valid _ Recorded). unfold import_history_record_valid in Valid. now rewrite Read, Key in Valid.
Qed.

Theorem import_history_commit_preserves_every_successful_callback :
  forall Hash rows initial records logical current observations storage_ok result event edges,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  In event logical ->
  import_history_logical_result event = ImportHistoryAccessResult (ImportCheckedHistoryFound edges) ->
  import_checked_wire_history Hash result (import_history_logical_key event) = Some edges.
Proof.
  intros Hash rows initial records logical current observations storage_ok result event edges Trace Commit Present Success.
  destruct (import_history_trace_exposes_only_physically_observed_checked_bytes
    _ _ _ _ _ _ _ _ _ Trace Present Success)
    as [bytes [record [Recorded [Key [Valid [Width [Domain [Hashed [Parsed Origin]]]]]]]]].
  assert (result (import_history_logical_key event) = Some bytes) as Stored.
  { destruct Origin as [[Received _]|[Fallback Found]].
    - pose proof (import_history_batch_writes_only_authenticated_received_bytes _ _ _ _ _ _ Commit) as Written.
      rewrite Forall_forall in Written. exact (proj2 (Written _ Received)).
    - pose proof (import_history_trace_preserves_all_successfully_observed_bytes _ _ _ _ _ _ _ Trace) as Retained.
      rewrite Forall_forall in Retained. specialize (Retained _ Recorded bytes Found).
      rewrite Key in Retained. eapply import_history_batch_preserves_all_existing_bytes; eauto. }
  assert (import_cursor_word_validb (import_history_logical_key event) = true) as KeyValid.
  { apply import_cursor_word_validity_characterization. auto. }
  unfold import_checked_wire_history. rewrite KeyValid, Stored, Hashed.
  destruct (list_eq_dec Nat.eq_dec (import_history_logical_key event) (import_history_logical_key event));
    [exact Parsed|contradiction].
Qed.

Theorem import_physical_history_extension_preserves_the_decoded_projection :
  forall first last,
  import_history_store_extension first last ->
  import_map_extension (import_project_wire_history first) (import_project_wire_history last).
Proof.
  intros first last Extended key edges Read. unfold import_project_wire_history in *.
  destruct (first (import_nat_to_key 32 key)) as [bytes|] eqn:Stored; [|discriminate].
  now rewrite (Extended _ _ Stored).
Qed.

Theorem import_observed_history_batch_preserves_joint_history_and_cold_bindings :
  forall Hash observations current rows storage_ok result success first_cold last_cold,
  import_observed_history_batch Hash observations current rows storage_ok = (result, success) ->
  import_encoded_run first_cold last_cold ->
  import_binding_extension
    (import_encoded_logical_view (import_project_wire_history current) first_cold)
    (import_encoded_logical_view (import_project_wire_history result) last_cold).
Proof.
  intros. apply import_encoded_view_preserves_joint_history_and_cold_extensions; [|assumption].
  apply import_physical_history_extension_preserves_the_decoded_projection.
  eapply import_history_batch_preserves_all_existing_bytes; eauto.
Qed.

Theorem import_observed_history_batch_extends_the_joint_storage_run :
  forall Hash observations initial_history initial_cold current cold rows storage_ok result success,
  import_encoded_storage_run initial_history initial_cold (import_project_wire_history current) cold ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, success) ->
  import_encoded_storage_run initial_history initial_cold (import_project_wire_history result) cold.
Proof.
  intros. eapply import_encoded_storage_history_extension; [eassumption|].
  apply import_physical_history_extension_preserves_the_decoded_projection.
  eapply import_history_batch_preserves_all_existing_bytes; eauto.
Qed.

Theorem import_observed_history_and_cold_batches_refine_a_consuming_scan_write :
  forall Value decode_item history_hash payload_hash Hash observations current rows storage_ok result success
    first_cold last_cold checked frontier,
  import_observed_history_batch Hash observations current rows storage_ok = (result, success) ->
  import_encoded_run first_cold last_cold ->
  import_consumer_scan_step Value decode_item history_hash payload_hash
    {| import_scan_store := import_encoded_logical_view (import_project_wire_history current) first_cold;
       import_scan_checked := checked; import_scan_frontier := frontier |}
    {| import_scan_store := import_encoded_logical_view (import_project_wire_history result) last_cold;
       import_scan_checked := checked; import_scan_frontier := frontier |}.
Proof.
  intros. apply import_consumer_scan_concurrent_commit.
  eapply import_observed_history_batch_preserves_joint_history_and_cold_bindings; eauto.
Qed.

Theorem import_observed_history_and_cold_batches_preserve_every_consumable_root :
  forall Value decode_item history_hash payload_hash Hash observations current rows storage_ok result success
    first_cold last_cold roots,
  import_observed_history_batch Hash observations current rows storage_ok = (result, success) ->
  import_encoded_run first_cold last_cold ->
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash
    (import_encoded_logical_view (import_project_wire_history current) first_cold)) roots ->
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash
    (import_encoded_logical_view (import_project_wire_history result) last_cold)) roots.
Proof.
  intros Value decode_item history_hash payload_hash Hash observations current rows storage_ok result success
    first_cold last_cold roots Commit Cold Closed.
  pose proof (import_observed_history_batch_preserves_joint_history_and_cold_bindings
    _ _ _ _ _ _ _ _ _ Commit Cold) as Bindings.
  eapply Forall_impl; [|exact Closed]. intros root Valid.
  eapply import_binding_extension_preserves_consumable_closed_root; eauto.
Qed.

Definition import_history_demo_key : list nat := repeat 0 32.
Definition import_history_demo_hash (_ : list nat) : list nat := import_history_demo_key.
Definition import_history_demo_store : ImportHistoryByteStore :=
  fun key => if list_eq_dec Nat.eq_dec key import_history_demo_key then Some [] else None.
Definition import_history_demo_observations : ImportHistoryObservations :=
  import_capture_history_observation import_empty_history_observations import_history_demo_key (ImportRootLookupFound []).
Definition import_history_demo_read : ImportHistoryReadRecord :=
  {| import_history_read_key := import_history_demo_key; import_history_read_store := import_history_demo_store;
     import_history_read_result := ImportRootLookupFound [] |}.
Definition import_history_demo_logical : ImportHistoryLogicalRead :=
  {| import_history_logical_key := import_history_demo_key; import_history_logical_cache := import_history_demo_observations;
     import_history_logical_result := ImportHistoryAccessResult (ImportCheckedHistoryFound []) |}.

Definition import_history_commit_demo_empty : ImportHistoryByteStore := fun _ => None.
Definition import_history_commit_demo_absent : ImportHistoryObservations :=
  import_capture_history_observation import_empty_history_observations import_history_demo_key ImportRootLookupAbsent.

Example import_history_commit_accepts_concurrent_identical_insertion :
  let result := import_observed_history_batch import_history_demo_hash import_history_commit_demo_absent
    import_history_demo_store [(import_history_demo_key, [])] true in
  snd result = true /\ fst result import_history_demo_key = Some [].
Proof. vm_compute. auto. Qed.

Example import_history_commit_rejects_concurrent_conflicting_insertion :
  let current := fun key => if list_eq_dec Nat.eq_dec key import_history_demo_key then Some [99] else None in
  let result := import_observed_history_batch import_history_demo_hash import_history_commit_demo_absent
    current [(import_history_demo_key, [])] true in
  snd result = false /\ fst result import_history_demo_key = Some [99].
Proof. vm_compute. auto. Qed.

Example import_history_commit_accepts_identical_duplicates :
  let result := import_observed_history_batch import_history_demo_hash import_history_commit_demo_absent
    import_history_commit_demo_empty [(import_history_demo_key, []); (import_history_demo_key, [])] true in
  snd result = true /\ fst result import_history_demo_key = Some [].
Proof. vm_compute. auto. Qed.

Example import_history_commit_rejects_unread_observations :
  let result := import_observed_history_batch import_history_demo_hash import_empty_history_observations
    import_history_commit_demo_empty [(import_history_demo_key, [])] true in
  snd result = false /\ fst result import_history_demo_key = None.
Proof. vm_compute. auto. Qed.

Example import_history_commit_rejects_failed_observations :
  let observations := import_capture_history_observation import_empty_history_observations
    import_history_demo_key ImportRootLookupFailure in
  let result := import_observed_history_batch import_history_demo_hash observations
    import_history_commit_demo_empty [(import_history_demo_key, [])] true in
  snd result = false /\ fst result import_history_demo_key = None.
Proof. vm_compute. auto. Qed.

Definition import_history_commit_demo_second_bytes : list nat := [1; 0] ++ repeat 0 32.
Definition import_history_commit_demo_second_key : list nat := repeat 1 32.
Definition import_history_commit_demo_hash (bytes : list nat) : list nat :=
  match bytes with [] => import_history_demo_key | _ => import_history_commit_demo_second_key end.
Definition import_history_commit_demo_rows : list ImportReceivedHistoryRow :=
  [(import_history_demo_key, []); (import_history_commit_demo_second_key, import_history_commit_demo_second_bytes)].
Definition import_history_commit_demo_observations : ImportHistoryObservations :=
  import_capture_history_observation import_history_commit_demo_absent import_history_commit_demo_second_key
    ImportRootLookupAbsent.

Example import_history_late_transaction_conflict_discards_the_staged_prefix :
  let current := fun key => if list_eq_dec Nat.eq_dec key import_history_commit_demo_second_key then Some [99] else None in
  let result := import_observed_history_batch import_history_commit_demo_hash import_history_commit_demo_observations
    current import_history_commit_demo_rows true in
  snd result = false /\ fst result import_history_demo_key = None /\
    fst result import_history_commit_demo_second_key = Some [99].
Proof. vm_compute. auto. Qed.

Example import_history_definite_write_failure_discards_the_whole_batch :
  let result := import_observed_history_batch import_history_commit_demo_hash import_history_commit_demo_observations
    import_history_commit_demo_empty import_history_commit_demo_rows false in
  snd result = false /\ fst result import_history_demo_key = None /\
    fst result import_history_commit_demo_second_key = None.
Proof. vm_compute. auto. Qed.

Example import_history_multirow_success_preserves_unrelated_writes :
  let unrelated := repeat 2 32 in
  let current := fun key => if list_eq_dec Nat.eq_dec key unrelated then Some [99] else None in
  let result := import_observed_history_batch import_history_commit_demo_hash import_history_commit_demo_observations
    current import_history_commit_demo_rows true in
  snd result = true /\ fst result import_history_demo_key = Some [] /\
    fst result import_history_commit_demo_second_key = Some import_history_commit_demo_second_bytes /\
    fst result unrelated = Some [99].
Proof. vm_compute. auto. Qed.

Example import_history_demo_observation_is_an_actual_read :
  import_history_observation_trace import_history_demo_hash [] import_history_demo_store
    [import_history_demo_read] [] import_history_demo_store import_history_demo_observations.
Proof.
  change (import_history_observation_trace import_history_demo_hash [] import_history_demo_store
    ([] ++ [{| import_history_read_key := import_history_demo_key; import_history_read_store := import_history_demo_store;
      import_history_read_result := import_history_successful_lookup import_history_demo_store import_history_demo_key |}])
    [] import_history_demo_store
    (import_capture_history_observation import_empty_history_observations import_history_demo_key
      (import_history_successful_lookup import_history_demo_store import_history_demo_key))).
  eapply import_history_observation_success; [constructor|reflexivity].
Qed.

Example import_two_history_callbacks_reuse_one_physical_read :
  import_history_observation_trace import_history_demo_hash [] import_history_demo_store
    [import_history_demo_read] [import_history_demo_logical; import_history_demo_logical]
    import_history_demo_store import_history_demo_observations.
Proof.
  change (import_history_observation_trace import_history_demo_hash [] import_history_demo_store
    [import_history_demo_read]
    ([import_history_demo_logical] ++ [{| import_history_logical_key := import_history_demo_key;
      import_history_logical_cache := import_history_demo_observations;
      import_history_logical_result := import_history_cached_access import_history_demo_hash []
        import_history_demo_observations import_history_demo_key |}])
    import_history_demo_store import_history_demo_observations).
  eapply import_history_logical_lookup.
  change (import_history_observation_trace import_history_demo_hash [] import_history_demo_store
    [import_history_demo_read]
    ([] ++ [{| import_history_logical_key := import_history_demo_key;
      import_history_logical_cache := import_history_demo_observations;
      import_history_logical_result := import_history_cached_access import_history_demo_hash []
        import_history_demo_observations import_history_demo_key |}])
    import_history_demo_store import_history_demo_observations).
  eapply import_history_logical_lookup. exact import_history_demo_observation_is_an_actual_read.
Qed.

Example import_received_history_cannot_hide_a_failed_compatibility_read :
  let failed := import_capture_history_observation import_empty_history_observations import_history_demo_key
    ImportRootLookupFailure in
  import_history_cached_access import_history_demo_hash [(import_history_demo_key, [])] failed import_history_demo_key =
    ImportHistoryPreparationFailed.
Proof. vm_compute. reflexivity. Qed.

Example import_received_history_cannot_hide_conflicting_stored_bytes :
  let conflict := import_capture_history_observation import_empty_history_observations import_history_demo_key
    (ImportRootLookupFound [99]) in
  import_history_cached_access import_history_demo_hash [(import_history_demo_key, [])] conflict import_history_demo_key =
    ImportHistoryPreparationFailed.
Proof. vm_compute. reflexivity. Qed.

Example import_fallback_rejects_incorrect_hash_and_invalid_radix_bytes :
  let malformed := import_capture_history_observation import_empty_history_observations import_history_demo_key
    (ImportRootLookupFound [99]) in
  import_history_cached_access (fun _ => repeat 1 32) [] import_history_demo_observations import_history_demo_key =
    ImportHistoryAccessResult ImportCheckedHistoryInvalid /\
  import_history_cached_access import_history_demo_hash [] malformed import_history_demo_key =
    ImportHistoryAccessResult ImportCheckedHistoryInvalid.
Proof. vm_compute. auto. Qed.

Example import_fresh_attempt_can_observe_bytes_after_prior_absence :
  let absent := import_capture_history_observation import_empty_history_observations import_history_demo_key
    ImportRootLookupAbsent in
  let refreshed := import_capture_history_observation absent import_history_demo_key (ImportRootLookupFound []) in
  import_history_cached_access import_history_demo_hash [] refreshed import_history_demo_key =
    ImportHistoryAccessResult ImportCheckedHistoryAbsent /\
  import_history_cached_access import_history_demo_hash [] import_history_demo_observations import_history_demo_key =
    ImportHistoryAccessResult (ImportCheckedHistoryFound []).
Proof. vm_compute. auto. Qed.
