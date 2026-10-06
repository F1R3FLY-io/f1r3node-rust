From Stdlib Require Import List Arith Bool.
From CostAccountedRho Require Import StateImportClosure StateImportStorage StateImportWire
  StateImportCodec StateImportCold StateImportOccurrence StateImportEncodedCold StateImportReceivedCold.
Import ListNotations.

Inductive ImportColdObservation : Type :=
| ImportColdUnread
| ImportColdObservedAbsent
| ImportColdObservedBytes (bytes : list nat)
| ImportColdReadFailure.

Definition ImportColdObservations := ImportColdAlias -> nat -> ImportColdObservation.

Definition import_empty_cold_observations : ImportColdObservations := fun _ _ => ImportColdUnread.

Definition import_cold_lookup_observation (result : ImportRootLookup) : ImportColdObservation :=
  match result with
  | ImportRootLookupAbsent => ImportColdObservedAbsent
  | ImportRootLookupFound bytes => ImportColdObservedBytes bytes
  | ImportRootLookupFailure => ImportColdReadFailure
  end.

Definition import_capture_first_cold_observation (observations : ImportColdObservations)
    (alias : ImportColdAlias) (key : nat) (result : ImportRootLookup) : ImportColdObservations :=
  fun selected query =>
    if import_cold_alias_eq_dec selected alias then
      if Nat.eq_dec query key then
        match observations selected query with
        | ImportColdUnread => import_cold_lookup_observation result
        | previous => previous
        end
      else observations selected query
    else observations selected query.

Theorem import_first_observation_retains_every_recorded_result : forall observations alias key result,
  forall selected query, observations selected query <> ImportColdUnread ->
  import_capture_first_cold_observation observations alias key result selected query = observations selected query.
Proof.
  intros observations alias key result selected query Known.
  unfold import_capture_first_cold_observation.
  destruct (import_cold_alias_eq_dec selected alias); [destruct (Nat.eq_dec query key)|]; auto.
  destruct (observations selected query); congruence.
Qed.

Theorem import_first_observation_captures_the_actual_result : forall observations alias key result,
  observations alias key = ImportColdUnread ->
  import_capture_first_cold_observation observations alias key result alias key = import_cold_lookup_observation result.
Proof.
  intros observations alias key result Unread. unfold import_capture_first_cold_observation.
  destruct (import_cold_alias_eq_dec alias alias); [|contradiction].
  destruct (Nat.eq_dec key key); [|contradiction]. now rewrite Unread.
Qed.

Record ImportColdReadRecord : Type := {
  import_cold_read_alias : ImportColdAlias;
  import_cold_read_key : nat;
  import_cold_read_store : ImportEncodedColdStore;
  import_cold_read_result : ImportRootLookup
}.

Definition import_cold_read_record_valid (record : ImportColdReadRecord) : Prop :=
  match import_cold_read_result record with
  | ImportRootLookupAbsent =>
      import_cold_read_store record (import_cold_read_alias record) (import_cold_read_key record) = None
  | ImportRootLookupFound bytes =>
      import_cold_read_store record (import_cold_read_alias record) (import_cold_read_key record) = Some bytes
  | ImportRootLookupFailure => True
  end.

Definition import_successful_cold_lookup (store : ImportEncodedColdStore) (alias : ImportColdAlias) (key : nat)
    : ImportRootLookup :=
  match store alias key with None => ImportRootLookupAbsent | Some bytes => ImportRootLookupFound bytes end.

Inductive import_cold_observation_trace (initial : ImportEncodedColdStore) :
    list ImportColdReadRecord -> ImportEncodedColdStore -> ImportColdObservations -> Prop :=
| import_cold_observation_begin : import_cold_observation_trace initial [] initial import_empty_cold_observations
| import_cold_observation_success : forall records store observations alias key,
    import_cold_observation_trace initial records store observations ->
    observations alias key = ImportColdUnread ->
    import_cold_observation_trace initial
      (records ++ [{| import_cold_read_alias := alias; import_cold_read_key := key;
        import_cold_read_store := store; import_cold_read_result := import_successful_cold_lookup store alias key |}])
      store (import_capture_first_cold_observation observations alias key (import_successful_cold_lookup store alias key))
| import_cold_observation_failure : forall records store observations alias key,
    import_cold_observation_trace initial records store observations ->
    observations alias key = ImportColdUnread ->
    import_cold_observation_trace initial
      (records ++ [{| import_cold_read_alias := alias; import_cold_read_key := key;
        import_cold_read_store := store; import_cold_read_result := ImportRootLookupFailure |}])
      store (import_capture_first_cold_observation observations alias key ImportRootLookupFailure)
| import_cold_observation_write : forall records store observations next,
    import_cold_observation_trace initial records store observations -> import_encoded_run store next ->
    import_cold_observation_trace initial records next observations.

Theorem import_observation_trace_preserves_exact_read_states : forall initial records store observations,
  import_cold_observation_trace initial records store observations -> Forall import_cold_read_record_valid records.
Proof.
  intros initial records store observations Trace. induction Trace.
  - constructor.
  - apply Forall_app. split; [exact IHTrace|]. constructor; [|constructor].
    cbn [import_cold_read_record_valid import_cold_read_result import_cold_read_store
      import_cold_read_alias import_cold_read_key].
    unfold import_successful_cold_lookup. destruct (store alias key) eqn:Read; cbn; exact Read.
  - apply Forall_app. split; [exact IHTrace|]. constructor; [exact I|constructor].
  - exact IHTrace.
Qed.

Definition import_observation_has_origin (records : list ImportColdReadRecord)
    (observations : ImportColdObservations) : Prop :=
  forall alias key, observations alias key <> ImportColdUnread -> exists record,
    In record records /\ import_cold_read_alias record = alias /\ import_cold_read_key record = key /\
    observations alias key = import_cold_lookup_observation (import_cold_read_result record).

Theorem import_capture_extends_observation_origins : forall records observations alias key store result,
  import_observation_has_origin records observations -> observations alias key = ImportColdUnread ->
  import_observation_has_origin
    (records ++ [{| import_cold_read_alias := alias; import_cold_read_key := key;
      import_cold_read_store := store; import_cold_read_result := result |}])
    (import_capture_first_cold_observation observations alias key result).
Proof.
  intros records observations alias key store result Origins Unread selected query Known.
  destruct (import_cold_alias_eq_dec selected alias) as [Alias|OtherAlias];
    [destruct (Nat.eq_dec query key) as [Key|OtherKey]|].
  - subst selected query.
    exists {| import_cold_read_alias := alias; import_cold_read_key := key;
      import_cold_read_store := store; import_cold_read_result := result |}.
    split; [apply in_or_app; right; now left|]. split; [reflexivity|]. split; [reflexivity|].
    now apply import_first_observation_captures_the_actual_result.
  - assert (import_capture_first_cold_observation observations alias key result selected query = observations selected query)
      as Same.
    { unfold import_capture_first_cold_observation.
      destruct (import_cold_alias_eq_dec selected alias); [destruct (Nat.eq_dec query key)|]; congruence. }
    rewrite Same in Known. destruct (Origins _ _ Known) as [record [Present Rest]].
    exists record. split; [apply in_or_app; now left|now rewrite Same].
  - assert (import_capture_first_cold_observation observations alias key result selected query = observations selected query)
      as Same.
    { unfold import_capture_first_cold_observation.
      destruct (import_cold_alias_eq_dec selected alias); congruence. }
    rewrite Same in Known. destruct (Origins _ _ Known) as [record [Present Rest]].
    exists record. split; [apply in_or_app; now left|now rewrite Same].
Qed.

Theorem import_observation_trace_derives_every_observation : forall initial records store observations,
  import_cold_observation_trace initial records store observations -> import_observation_has_origin records observations.
Proof.
  intros initial records store observations Trace. induction Trace.
  - intros alias key Impossible. exfalso. now apply Impossible.
  - eapply import_capture_extends_observation_origins; eauto.
  - eapply import_capture_extends_observation_origins; eauto.
  - exact IHTrace.
Qed.

Definition import_records_retained (records : list ImportColdReadRecord) (observations : ImportColdObservations) : Prop :=
  Forall (fun record => observations (import_cold_read_alias record) (import_cold_read_key record) =
    import_cold_lookup_observation (import_cold_read_result record)) records.

Theorem import_capture_preserves_all_earlier_read_records : forall records observations alias key result,
  import_records_retained records observations ->
  import_records_retained records (import_capture_first_cold_observation observations alias key result).
Proof.
  intros records observations alias key result Retained.
  unfold import_records_retained in Retained.
  apply Forall_forall. intros record Present. rewrite Forall_forall in Retained. specialize (Retained _ Present).
  rewrite import_first_observation_retains_every_recorded_result; [exact Retained|].
  rewrite Retained. destruct (import_cold_read_result record); discriminate.
Qed.

Theorem import_observation_trace_retains_every_read_including_failures : forall initial records store observations,
  import_cold_observation_trace initial records store observations -> import_records_retained records observations.
Proof.
  intros initial records store observations Trace. induction Trace.
  - constructor.
  - apply Forall_app. split.
    + now apply import_capture_preserves_all_earlier_read_records.
    + constructor; [|constructor]. cbn [import_cold_read_alias import_cold_read_key import_cold_read_result].
      now apply import_first_observation_captures_the_actual_result.
  - apply Forall_app. split.
    + now apply import_capture_preserves_all_earlier_read_records.
    + constructor; [|constructor]. cbn [import_cold_read_alias import_cold_read_key import_cold_read_result].
      now apply import_first_observation_captures_the_actual_result.
  - exact IHTrace.
Qed.

Definition import_cold_read_location (record : ImportColdReadRecord) : ImportColdAlias * nat :=
  (import_cold_read_alias record, import_cold_read_key record).

Theorem import_unread_location_has_no_earlier_physical_read : forall records observations alias key,
  import_records_retained records observations -> observations alias key = ImportColdUnread ->
  ~ In (alias, key) (map import_cold_read_location records).
Proof.
  intros records observations alias key Retained Unread Present.
  apply in_map_iff in Present as [record [Location Present]].
  unfold import_records_retained in Retained. rewrite Forall_forall in Retained.
  specialize (Retained _ Present). unfold import_cold_read_location in Location.
  inversion Location as [[Alias Key]]. rewrite Alias, Key, Unread in Retained.
  destruct (import_cold_read_result record); discriminate.
Qed.

Theorem import_observation_trace_reads_each_physical_location_once : forall initial records store observations,
  import_cold_observation_trace initial records store observations ->
  NoDup (map import_cold_read_location records).
Proof.
  intros initial records store observations Trace. induction Trace.
  - constructor.
  - rewrite map_app. apply NoDup_app; [exact IHTrace| |].
    + constructor; [simpl; tauto|constructor].
    + intros location Earlier New. simpl in New. destruct New as [Same|Impossible]; [|contradiction].
      subst location. eapply import_unread_location_has_no_earlier_physical_read; eauto.
      eapply import_observation_trace_retains_every_read_including_failures; eauto.
  - rewrite map_app. apply NoDup_app; [exact IHTrace| |].
    + constructor; [simpl; tauto|constructor].
    + intros location Earlier New. simpl in New. destruct New as [Same|Impossible]; [|contradiction].
      subst location. eapply import_unread_location_has_no_earlier_physical_read; eauto.
      eapply import_observation_trace_retains_every_read_including_failures; eauto.
  - exact IHTrace.
Qed.

Theorem import_observation_trace_preserves_the_concurrent_storage_run : forall initial records current observations,
  import_cold_observation_trace initial records current observations -> import_encoded_run initial current.
Proof.
  intros initial records current observations Trace. induction Trace; try assumption; [constructor|].
  eapply import_encoded_run_transitive; eauto.
Qed.

Definition import_cold_observation_ready (observation : ImportColdObservation) : bool :=
  match observation with ImportColdObservedAbsent | ImportColdObservedBytes _ => true | _ => false end.

Definition import_project_cold_observations (observations : ImportColdObservations) : ImportEncodedColdStore :=
  fun alias key => match observations alias key with ImportColdObservedBytes bytes => Some bytes | _ => None end.

Definition import_cold_row_observations_ready (observations : ImportColdObservations) (row : ImportEncodedRow) : bool :=
  match row with import_encoded_row _ key _ =>
    import_cold_observation_ready (observations ImportRawAlias key) &&
    import_cold_observation_ready (observations ImportLegacyAlias key)
  end.

Definition import_observed_physical_cold_batch (observations : ImportColdObservations)
    (current : ImportEncodedColdStore) (rows : list ImportEncodedRow) (storage_ok : bool)
    : ImportEncodedColdStore * bool :=
  if forallb (import_cold_row_observations_ready observations) rows
  then import_observed_encoded_batch (import_project_cold_observations observations) current rows storage_ok
  else (current, false).

Theorem import_physical_batch_success_requires_both_recorded_aliases :
  forall observations current rows storage_ok result,
  import_observed_physical_cold_batch observations current rows storage_ok = (result, true) ->
  Forall (fun row => import_cold_row_observations_ready observations row = true) rows /\
  import_observed_encoded_batch (import_project_cold_observations observations) current rows storage_ok = (result, true).
Proof.
  intros observations current rows storage_ok result Committed.
  unfold import_observed_physical_cold_batch in Committed.
  destruct (forallb _ rows) eqn:Ready; [|discriminate]. split; [|exact Committed].
  apply Forall_forall. intros row Present. apply forallb_forall with (x := row) in Ready; auto.
Qed.

Theorem import_accepted_physical_batch_has_read_evidence_for_each_alias :
  forall initial records observed_store observations current rows storage_ok result alias key bytes selected,
  import_cold_observation_trace initial records observed_store observations ->
  import_observed_physical_cold_batch observations current rows storage_ok = (result, true) ->
  In (import_encoded_row alias key bytes) rows ->
  exists record,
    In record records /\ import_cold_read_alias record = selected /\ import_cold_read_key record = key /\
    import_cold_read_record_valid record /\
    import_cold_read_result record <> ImportRootLookupFailure /\
    import_project_cold_observations observations selected key =
      import_cold_read_store record selected key.
Proof.
  intros initial records observed_store observations current rows storage_ok result alias key bytes selected
    Trace Committed Present.
  apply import_physical_batch_success_requires_both_recorded_aliases in Committed as [Ready _].
  rewrite Forall_forall in Ready. specialize (Ready _ Present).
  cbn [import_cold_row_observations_ready] in Ready. apply andb_true_iff in Ready as [Raw Legacy].
  assert (import_cold_observation_ready (observations selected key) = true) as Selected
    by (destruct selected; assumption).
  assert (observations selected key <> ImportColdUnread) as Known
    by (intros Wrong; rewrite Wrong in Selected; discriminate).
  destruct (import_observation_trace_derives_every_observation _ _ _ _ Trace _ _ Known)
    as [record [InRecords [Alias [Key Observation]]]].
  pose proof (import_observation_trace_preserves_exact_read_states _ _ _ _ Trace) as Records.
  rewrite Forall_forall in Records. specialize (Records _ InRecords).
  exists record. split; [exact InRecords|]. split; [exact Alias|]. split; [exact Key|].
  split; [exact Records|].
  unfold import_project_cold_observations. rewrite Observation.
  rewrite Observation in Selected.
  unfold import_cold_read_record_valid in Records. rewrite Alias, Key in Records.
  destruct (import_cold_read_result record); cbn in *; try discriminate; split; congruence.
Qed.

Theorem import_physical_batch_never_treats_unread_or_failed_aliases_as_absent :
  forall observations current rows storage_ok result alias key bytes selected,
  In (import_encoded_row alias key bytes) rows ->
  (observations selected key = ImportColdUnread \/ observations selected key = ImportColdReadFailure) ->
  import_observed_physical_cold_batch observations current rows storage_ok <> (result, true).
Proof.
  intros observations current rows storage_ok result alias key bytes selected Present Invalid Committed.
  apply import_physical_batch_success_requires_both_recorded_aliases in Committed as [Ready _].
  rewrite Forall_forall in Ready. specialize (Ready _ Present).
  cbn [import_cold_row_observations_ready] in Ready. apply andb_true_iff in Ready as [Raw Legacy].
  assert (import_cold_observation_ready (observations selected key) = true) as Selected
    by (destruct selected; assumption).
  destruct Invalid as [Unread|Failed]; [rewrite Unread in Selected|rewrite Failed in Selected]; discriminate.
Qed.

Theorem import_failed_physical_batch_preserves_current_storage : forall observations current rows storage_ok result,
  import_observed_physical_cold_batch observations current rows storage_ok = (result, false) -> result = current.
Proof.
  intros observations current rows storage_ok result Failed. unfold import_observed_physical_cold_batch in Failed.
  destruct (forallb _ rows); [|now inversion Failed].
  destruct (import_observed_batches_preserve_definite_rollback_and_guarded_progress _ _ _ _ _ _ Failed) as [_ Rollback].
  now apply Rollback.
Qed.

Theorem import_any_physical_batch_preserves_existing_bytes : forall observations current rows storage_ok result success,
  import_observed_physical_cold_batch observations current rows storage_ok = (result, success) ->
  import_encoded_extension current result.
Proof.
  intros observations current rows storage_ok result success Attempt. destruct success.
  - apply import_physical_batch_success_requires_both_recorded_aliases in Attempt as [_ Attempt].
    destruct (import_observed_batches_preserve_definite_rollback_and_guarded_progress _ _ _ _ _ _ Attempt) as [Run _].
    destruct (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings (fun _ => None) _ _ Run) as [Bytes _].
    exact Bytes.
  - apply import_failed_physical_batch_preserves_current_storage in Attempt. subst result.
    intros alias key bytes Found. exact Found.
Qed.

Theorem import_physical_commit_rechecks_each_alias_against_current_storage :
  forall observations current rows storage_ok result alias key bytes selected,
  import_observed_physical_cold_batch observations current rows storage_ok = (result, true) ->
  In (import_encoded_row alias key bytes) rows ->
  import_project_cold_observations observations selected key = current selected key.
Proof.
  intros observations current rows storage_ok result alias key bytes selected Committed Present.
  apply import_physical_batch_success_requires_both_recorded_aliases in Committed as [_ Committed].
  unfold import_observed_encoded_batch in Committed.
  destruct (import_stage_encoded_batch _ rows); [|discriminate].
  destruct (forallb _ rows && storage_ok) eqn:Matched; [|discriminate].
  apply andb_true_iff in Matched as [All _].
  apply forallb_forall with (x := import_encoded_row alias key bytes) in All; [|exact Present].
  cbn [import_encoded_row_observation_matches] in All.
  apply import_encoded_observations_match_exactly_both_aliases in All as [Raw Legacy].
  destruct selected; assumption.
Qed.

Theorem import_required_failed_record_prevents_later_commit :
  forall initial records observed_store observations current rows storage_ok result record alias key bytes,
  import_cold_observation_trace initial records observed_store observations -> In record records ->
  import_cold_read_result record = ImportRootLookupFailure -> import_cold_read_key record = key ->
  In (import_encoded_row alias key bytes) rows ->
  import_observed_physical_cold_batch observations current rows storage_ok <> (result, true).
Proof.
  intros initial records observed_store observations current rows storage_ok result record alias key bytes
    Trace Present Failed Key Row.
  pose proof (import_observation_trace_retains_every_read_including_failures _ _ _ _ Trace) as Retained.
  unfold import_records_retained in Retained.
  rewrite Forall_forall in Retained. specialize (Retained _ Present). rewrite Failed, Key in Retained.
  eapply import_physical_batch_never_treats_unread_or_failed_aliases_as_absent; [exact Row|].
  right. exact Retained.
Qed.

Theorem import_recorded_bytes_match_the_commit_state_without_a_common_read_snapshot :
  forall initial records observed_store observations current rows storage_ok result alias key bytes selected,
  import_cold_observation_trace initial records observed_store observations ->
  import_observed_physical_cold_batch observations current rows storage_ok = (result, true) ->
  In (import_encoded_row alias key bytes) rows ->
  exists record, In record records /\ import_cold_read_alias record = selected /\
    import_cold_read_key record = key /\ import_cold_read_record_valid record /\
    import_cold_read_result record <> ImportRootLookupFailure /\
    current selected key = import_cold_read_store record selected key.
Proof.
  intros initial records observed_store observations current rows storage_ok result alias key bytes selected
    Trace Committed Present.
  destruct (import_accepted_physical_batch_has_read_evidence_for_each_alias
    _ _ _ _ _ _ _ _ _ _ _ selected Trace Committed Present)
    as [record [Included [Alias [Key [Valid [Success Bytes]]]]]].
  exists record. repeat split; try assumption.
  rewrite <- (import_physical_commit_rechecks_each_alias_against_current_storage
    _ _ _ _ _ _ _ _ selected Committed Present). exact Bytes.
Qed.

Section PhysicalColdConsumption.

Variable Value : ImportLeafKind -> Type.
Variable decode_item : forall kind, list nat -> option (Value kind).
Variable HashBytes : list nat -> list nat.

Theorem import_received_rows_with_physical_preflight_preserve_bytes_and_typed_consumption :
  forall entries rows candidate observations current storage_ok result history history_hash,
  import_check_received_cold_page Value decode_item HashBytes entries rows = Some candidate ->
  import_observed_physical_cold_batch observations current (map import_compile_received_cold_row rows) storage_ok
    = (result, true) ->
  import_encoded_extension current result /\
  (forall entry key kind, In entry entries -> import_located_reference entry = Some (ImportColdRef key kind) ->
    import_consumer_checked_read Value decode_item history_hash
      (fun payload => import_received_hash HashBytes (import_nat_to_key 8 (length payload) ++ payload))
      (import_encoded_logical_view history result) (ImportColdRef key kind) = Some []).
Proof.
  intros entries rows candidate observations current storage_ok result history history_hash Checked Committed.
  split; [eapply import_any_physical_batch_preserves_existing_bytes; exact Committed|].
  intros entry key kind Present Reference.
  apply import_physical_batch_success_requires_both_recorded_aliases in Committed as [_ Committed].
  eapply import_received_batch_establishes_every_authorized_occurrence; eauto.
Qed.

Theorem import_received_rows_with_actual_read_trace_preserve_bytes_provenance_and_typed_consumption :
  forall entries rows candidate initial records observations current storage_ok result history history_hash,
  import_check_received_cold_page Value decode_item HashBytes entries rows = Some candidate ->
  import_cold_observation_trace initial records current observations ->
  import_observed_physical_cold_batch observations current (map import_compile_received_cold_row rows) storage_ok
    = (result, true) ->
  import_encoded_run initial current /\
  NoDup (map import_cold_read_location records) /\
  import_encoded_extension current result /\
  (forall alias key bytes selected, In (import_encoded_row alias key bytes) (map import_compile_received_cold_row rows) ->
    exists record, In record records /\ import_cold_read_alias record = selected /\
      import_cold_read_key record = key /\ import_cold_read_record_valid record /\
      import_cold_read_result record <> ImportRootLookupFailure /\
      current selected key = import_cold_read_store record selected key) /\
  (forall entry key kind, In entry entries -> import_located_reference entry = Some (ImportColdRef key kind) ->
    import_consumer_checked_read Value decode_item history_hash
      (fun payload => import_received_hash HashBytes (import_nat_to_key 8 (length payload) ++ payload))
      (import_encoded_logical_view history result) (ImportColdRef key kind) = Some []).
Proof.
  intros entries rows candidate initial records observations current storage_ok result history history_hash
    Checked Trace Committed.
  destruct (import_received_rows_with_physical_preflight_preserve_bytes_and_typed_consumption
    _ _ _ _ _ _ _ history history_hash Checked Committed) as [Preserved Consumed].
  split; [eapply import_observation_trace_preserves_the_concurrent_storage_run; exact Trace|].
  split; [eapply import_observation_trace_reads_each_physical_location_once; exact Trace|].
  split; [exact Preserved|]. split; [|exact Consumed].
  intros alias key bytes selected Present.
  eapply import_recorded_bytes_match_the_commit_state_without_a_common_read_snapshot; eauto.
Qed.

End PhysicalColdConsumption.

Theorem import_required_read_failure_cannot_be_erased_by_a_later_success :
  forall observations alias key bytes,
  observations alias key = ImportColdReadFailure ->
  import_capture_first_cold_observation observations alias key (ImportRootLookupFound bytes) alias key = ImportColdReadFailure.
Proof.
  intros observations alias key bytes Failed.
  rewrite import_first_observation_retains_every_recorded_result; [exact Failed|rewrite Failed; discriminate].
Qed.

Theorem import_same_touched_observations_have_equal_commit_guards : forall first second current rows,
  Forall (fun row => import_encoded_row_observation_matches first second row = true) rows ->
  forallb (import_encoded_row_observation_matches first current) rows =
    forallb (import_encoded_row_observation_matches second current) rows.
Proof.
  intros first second current rows Same. induction Same as [|[alias key bytes] rest Head Tail IH];
    cbn [forallb]; [reflexivity|].
  cbn [import_encoded_row_observation_matches] in *.
  apply import_encoded_observations_match_exactly_both_aliases in Head as [Raw Legacy].
  rewrite IH. unfold import_encoded_observations_match. now rewrite Raw, Legacy.
Qed.

Theorem import_observed_batches_ignore_untouched_observation_values : forall first second current rows storage_ok,
  Forall (fun row => import_encoded_row_observation_matches first second row = true) rows ->
  import_observed_encoded_batch first current rows storage_ok =
    import_observed_encoded_batch second current rows storage_ok.
Proof.
  intros first second current rows storage_ok Same.
  assert (Forall (fun row => import_encoded_row_observation_matches second first row = true) rows) as Reverse.
  { eapply Forall_impl; [|exact Same]. intros [alias key bytes] Equal.
    cbn [import_encoded_row_observation_matches] in *.
    apply import_encoded_observations_match_exactly_both_aliases in Equal as [Raw Legacy].
    apply import_encoded_observations_match_exactly_both_aliases. split; symmetry; assumption. }
  unfold import_observed_encoded_batch.
  destruct (import_stage_encoded_batch first rows) as [first_candidate|] eqn:First;
    destruct (import_stage_encoded_batch second rows) as [second_candidate|] eqn:Second.
  - now rewrite (import_same_touched_observations_have_equal_commit_guards _ _ _ _ Same).
  - destruct (import_checked_batch_observations_transfer_staging _ _ _ _ First Same) as [candidate Staged]. congruence.
  - destruct (import_checked_batch_observations_transfer_staging _ _ _ _ Second Reverse) as [candidate Staged]. congruence.
  - reflexivity.
Qed.

Definition import_control_absent_observations : ImportColdObservations :=
  import_capture_first_cold_observation
    (import_capture_first_cold_observation import_empty_cold_observations ImportRawAlias 0 ImportRootLookupAbsent)
    ImportLegacyAlias 0 ImportRootLookupAbsent.

Example import_control_absence_comes_from_two_actual_read_steps :
  exists records, import_cold_observation_trace import_empty_received_cold records import_empty_received_cold
    import_control_absent_observations.
Proof.
  unfold import_control_absent_observations. eexists.
  eapply import_cold_observation_success.
  - eapply import_cold_observation_success; [constructor|reflexivity].
  - reflexivity.
Qed.

Example import_unread_opposite_alias_blocks_preparation :
  let observations := import_capture_first_cold_observation import_empty_cold_observations
    ImportRawAlias 0 ImportRootLookupAbsent in
  import_cold_row_observations_ready observations
    (import_encoded_row ImportRawAlias 0 import_valid_empty_joins_bytes) = false.
Proof. vm_compute. reflexivity. Qed.

Example import_failed_alias_blocks_preparation_despite_absent_projection :
  let observations := fun alias _ => match alias with
    | ImportRawAlias => ImportColdObservedAbsent | ImportLegacyAlias => ImportColdReadFailure end in
  import_project_cold_observations observations ImportLegacyAlias 0 = None /\
  import_observed_physical_cold_batch observations import_empty_received_cold
    [import_encoded_row ImportRawAlias 0 import_valid_empty_joins_bytes] true = (import_empty_received_cold, false).
Proof. vm_compute. auto. Qed.

Example import_opposite_alias_insertion_invalidates_stale_observations :
  let current := import_fill_encoded_alias import_empty_received_cold ImportLegacyAlias 0 import_valid_empty_joins_bytes in
  import_observed_physical_cold_batch import_control_absent_observations current
    [import_encoded_row ImportRawAlias 0 import_valid_empty_joins_bytes] true = (current, false).
Proof. vm_compute. reflexivity. Qed.

Example import_unrelated_existing_bytes_survive_successful_physical_batch :
  let current := import_fill_encoded_alias import_empty_received_cold ImportRawAlias 1 [99] in
  let '(result, success) := import_observed_physical_cold_batch import_control_absent_observations current
    [import_encoded_row ImportRawAlias 0 import_valid_empty_joins_bytes] true in
  success = true /\ result ImportRawAlias 1 = Some [99] /\
    result ImportRawAlias 0 = Some import_valid_empty_joins_bytes.
Proof. vm_compute. auto. Qed.

Definition import_control_unrelated_cold_insert : ImportEncodedColdStore :=
  import_fill_encoded_alias import_empty_received_cold ImportRawAlias 1 import_valid_empty_joins_bytes.

Definition import_control_interleaved_cold_read_records : list ImportColdReadRecord :=
  [{| import_cold_read_alias := ImportRawAlias; import_cold_read_key := 0;
      import_cold_read_store := import_empty_received_cold; import_cold_read_result := ImportRootLookupAbsent |};
   {| import_cold_read_alias := ImportLegacyAlias; import_cold_read_key := 0;
      import_cold_read_store := import_control_unrelated_cold_insert; import_cold_read_result := ImportRootLookupAbsent |}].

Example import_compatible_unrelated_write_between_alias_reads_is_an_actual_trace :
  import_cold_observation_trace import_empty_received_cold import_control_interleaved_cold_read_records
    import_control_unrelated_cold_insert import_control_absent_observations.
Proof.
  change (import_cold_observation_trace import_empty_received_cold
    ([{| import_cold_read_alias := ImportRawAlias; import_cold_read_key := 0;
         import_cold_read_store := import_empty_received_cold; import_cold_read_result := ImportRootLookupAbsent |}] ++
     [{| import_cold_read_alias := ImportLegacyAlias; import_cold_read_key := 0;
         import_cold_read_store := import_control_unrelated_cold_insert;
         import_cold_read_result := import_successful_cold_lookup import_control_unrelated_cold_insert ImportLegacyAlias 0 |}])
    import_control_unrelated_cold_insert
    (import_capture_first_cold_observation
      (import_capture_first_cold_observation import_empty_cold_observations ImportRawAlias 0 ImportRootLookupAbsent)
      ImportLegacyAlias 0 (import_successful_cold_lookup import_control_unrelated_cold_insert ImportLegacyAlias 0))).
  eapply import_cold_observation_success; [|reflexivity].
  eapply import_cold_observation_write with (store := import_empty_received_cold).
  - change (import_cold_observation_trace import_empty_received_cold
      ([] ++ [{| import_cold_read_alias := ImportRawAlias; import_cold_read_key := 0;
        import_cold_read_store := import_empty_received_cold;
        import_cold_read_result := import_successful_cold_lookup import_empty_received_cold ImportRawAlias 0 |}])
      import_empty_received_cold
      (import_capture_first_cold_observation import_empty_cold_observations ImportRawAlias 0
        (import_successful_cold_lookup import_empty_received_cold ImportRawAlias 0))).
    eapply import_cold_observation_success; [constructor|reflexivity].
  - eapply import_encoded_run_step with (middle := import_empty_received_cold)
      (alias := ImportRawAlias) (key := 1) (input := import_valid_empty_joins_bytes)
      (storage_ok := true) (success := true); [constructor|].
    vm_compute. reflexivity.
Qed.

Example import_compatible_interleaved_write_survives_successful_physical_batch :
  let '(result, success) := import_observed_physical_cold_batch import_control_absent_observations
    import_control_unrelated_cold_insert [import_encoded_row ImportRawAlias 0 import_valid_empty_joins_bytes] true in
  success = true /\ result ImportRawAlias 1 = Some import_valid_empty_joins_bytes /\
    result ImportRawAlias 0 = Some import_valid_empty_joins_bytes.
Proof. vm_compute. auto. Qed.

Example import_later_success_cannot_clear_a_required_failure :
  let failed := import_capture_first_cold_observation import_empty_cold_observations
    ImportRawAlias 0 ImportRootLookupFailure in
  let retried := import_capture_first_cold_observation failed ImportRawAlias 0
    (ImportRootLookupFound import_valid_empty_joins_bytes) in
  retried ImportRawAlias 0 = ImportColdReadFailure /\
    import_cold_row_observations_ready retried (import_encoded_row ImportRawAlias 0 import_valid_empty_joins_bytes) = false.
Proof. vm_compute. auto. Qed.
