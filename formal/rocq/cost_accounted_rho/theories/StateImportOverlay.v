From Stdlib Require Import List Arith Bool Lia Sorting.Permutation.
From CostAccountedRho Require Import StateImportCodec StateImportCursor StateImportTraversal
  StateImportPage StateImportStack StateImportExport StateImportExecution StateImportWire StateImportOccurrence.
Import ListNotations.

Definition ImportFallibleHistory := list nat -> ImportRootLookup.
Definition ImportReceivedHistoryRow := (list nat * list nat)%type.

Definition import_check_received_history_row (Hash : list nat -> list nat)
    (key bytes : list nat) : option (list ImportWireEdge) :=
  import_checked_wire_history Hash (fun _ => Some bytes) key.

Theorem import_received_history_check_authenticates_bytes : forall Hash key bytes edges,
  import_check_received_history_row Hash key bytes = Some edges ->
  length key = 32 /\ Forall import_wire_byte key /\ Hash bytes = key /\
    import_parse_wire_node bytes = Some edges.
Proof.
  intros Hash key bytes edges Checked.
  apply import_checked_wire_history_has_exact_witness in Checked
    as [Width [Domain [stored [Read [Hashed Parsed]]]]].
  inversion Read; subst stored. auto.
Qed.

Definition import_history_observation_extension (first last : ImportFallibleHistory) : Prop :=
  forall key bytes, first key = ImportRootLookupFound bytes -> last key = ImportRootLookupFound bytes.

Definition import_candidate_history_insert (before : ImportFallibleHistory) (key bytes : list nat)
    : ImportFallibleHistory :=
  fun query => if list_eq_dec Nat.eq_dec query key then ImportRootLookupFound bytes else before query.

Inductive ImportHistoryOverlayResult : Type :=
| ImportHistoryOverlayReady (candidate : ImportFallibleHistory)
| ImportHistoryOverlayInvalid (key : list nat)
| ImportHistoryOverlayConflict (key : list nat)
| ImportHistoryOverlayStorageFailure (key : list nat).

Definition import_stage_history_row (Hash : list nat -> list nat) (before : ImportFallibleHistory)
    (row : ImportReceivedHistoryRow) : ImportHistoryOverlayResult :=
  let '(key, bytes) := row in
  match import_check_received_history_row Hash key bytes with
  | None => ImportHistoryOverlayInvalid key
  | Some _ => match before key with
    | ImportRootLookupAbsent => ImportHistoryOverlayReady (import_candidate_history_insert before key bytes)
    | ImportRootLookupFailure => ImportHistoryOverlayStorageFailure key
    | ImportRootLookupFound existing =>
        if list_eq_dec Nat.eq_dec existing bytes then ImportHistoryOverlayReady before
        else ImportHistoryOverlayConflict key
    end
  end.

Theorem import_staged_history_row_preserves_observed_bytes : forall Hash before row after,
  import_stage_history_row Hash before row = ImportHistoryOverlayReady after ->
  import_history_observation_extension before after.
Proof.
  intros Hash before [key bytes] after Staged query existing Read.
  unfold import_stage_history_row in Staged.
  destruct (import_check_received_history_row Hash key bytes); [|discriminate].
  destruct (before key) as [|stored|] eqn:Old; [| |discriminate].
  - inversion Staged; subst after. unfold import_candidate_history_insert.
    destruct (list_eq_dec Nat.eq_dec query key) as [Same|Different]; auto.
    subst query. congruence.
  - destruct (list_eq_dec Nat.eq_dec stored bytes); [|discriminate].
    inversion Staged; subst after. exact Read.
Qed.

Theorem import_staged_history_row_has_authenticated_binding : forall Hash before key bytes after,
  import_stage_history_row Hash before (key, bytes) = ImportHistoryOverlayReady after ->
  (exists edges, import_check_received_history_row Hash key bytes = Some edges) /\
    after key = ImportRootLookupFound bytes.
Proof.
  intros Hash before key bytes after Staged. unfold import_stage_history_row in Staged.
  destruct (import_check_received_history_row Hash key bytes) as [edges|] eqn:Checked; [|discriminate].
  split; [eauto|]. destruct (before key) as [|stored|] eqn:Old; [| |discriminate].
  - inversion Staged; subst after. unfold import_candidate_history_insert.
    destruct (list_eq_dec Nat.eq_dec key key); congruence.
  - destruct (list_eq_dec Nat.eq_dec stored bytes); [|discriminate].
    inversion Staged; subst after. congruence.
Qed.

Theorem import_staging_changes_only_the_received_key : forall Hash before row after query,
  import_stage_history_row Hash before row = ImportHistoryOverlayReady after ->
  query <> fst row -> after query = before query.
Proof.
  intros Hash before [key bytes] after query Staged Different.
  cbn [fst] in Different.
  unfold import_stage_history_row in Staged.
  destruct (import_check_received_history_row Hash key bytes); [|discriminate].
  destruct (before key) as [|stored|]; [| |discriminate].
  - inversion Staged; subst after. unfold import_candidate_history_insert.
    destruct (list_eq_dec Nat.eq_dec query key); congruence.
  - destruct (list_eq_dec Nat.eq_dec stored bytes); [|discriminate].
    inversion Staged; subst after. reflexivity.
Qed.

Theorem import_history_io_failure_is_not_an_absent_row : forall Hash before key bytes edges,
  import_check_received_history_row Hash key bytes = Some edges ->
  before key = ImportRootLookupFailure ->
  import_stage_history_row Hash before (key, bytes) = ImportHistoryOverlayStorageFailure key.
Proof. intros. unfold import_stage_history_row. now rewrite H, H0. Qed.

Theorem import_conflicting_observed_history_bytes_reject : forall Hash before key bytes edges existing,
  import_check_received_history_row Hash key bytes = Some edges ->
  before key = ImportRootLookupFound existing -> existing <> bytes ->
  import_stage_history_row Hash before (key, bytes) = ImportHistoryOverlayConflict key.
Proof.
  intros. unfold import_stage_history_row. rewrite H, H0.
  destruct (list_eq_dec Nat.eq_dec existing bytes); congruence.
Qed.

Theorem import_valid_compatible_history_row_can_be_staged : forall Hash before key bytes edges,
  import_check_received_history_row Hash key bytes = Some edges ->
  (before key = ImportRootLookupAbsent \/ before key = ImportRootLookupFound bytes) ->
  exists after, import_stage_history_row Hash before (key, bytes) = ImportHistoryOverlayReady after.
Proof.
  intros Hash before key bytes edges Checked Compatible. unfold import_stage_history_row.
  rewrite Checked. destruct Compatible as [Absent|Present]; rewrite ?Absent, ?Present.
  - eauto.
  - destruct (list_eq_dec Nat.eq_dec bytes bytes); [eauto|contradiction].
Qed.

Fixpoint import_prepare_history_overlay (Hash : list nat -> list nat) (before : ImportFallibleHistory)
    (rows : list ImportReceivedHistoryRow) : ImportHistoryOverlayResult :=
  match rows with
  | [] => ImportHistoryOverlayReady before
  | row :: rest => match import_stage_history_row Hash before row with
    | ImportHistoryOverlayReady candidate => import_prepare_history_overlay Hash candidate rest
    | failure => failure
    end
  end.

Theorem import_history_overlay_preserves_every_observed_binding : forall Hash rows before after,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady after ->
  import_history_observation_extension before after.
Proof.
  intros Hash rows. induction rows as [|row rest IH]; intros before after Prepared.
  - inversion Prepared; subst after. intros key bytes Read. exact Read.
  - cbn [import_prepare_history_overlay] in Prepared.
    destruct (import_stage_history_row Hash before row) as [candidate|key|key|key] eqn:Staged;
      try discriminate.
    intros key bytes Read. apply (IH candidate after Prepared).
    eapply import_staged_history_row_preserves_observed_bytes; eauto.
Qed.

Theorem import_history_overlay_authenticates_every_received_row : forall Hash rows before after,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady after ->
  Forall (fun row =>
    (exists edges, import_check_received_history_row Hash (fst row) (snd row) = Some edges) /\
    after (fst row) = ImportRootLookupFound (snd row)) rows.
Proof.
  intros Hash rows. induction rows as [|[key bytes] rest IH]; intros before after Prepared.
  - constructor.
  - cbn [import_prepare_history_overlay] in Prepared.
    destruct (import_stage_history_row Hash before (key, bytes)) as [candidate|invalid|conflict|failed] eqn:Staged;
      try discriminate.
    constructor; [|eapply IH; eauto].
    destruct (import_staged_history_row_has_authenticated_binding _ _ _ _ _ Staged) as [Checked Read].
    split; [exact Checked|].
    eapply import_history_overlay_preserves_every_observed_binding; eauto.
Qed.

Theorem import_history_overlay_does_not_change_unreceived_keys : forall Hash rows before after key,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady after ->
  ~ In key (map fst rows) -> after key = before key.
Proof.
  intros Hash rows. induction rows as [|row rest IH]; intros before after key Prepared Absent.
  - inversion Prepared; subst after. reflexivity.
  - cbn [import_prepare_history_overlay] in Prepared.
    destruct (import_stage_history_row Hash before row) as [candidate|invalid|conflict|failed] eqn:Staged;
      try discriminate.
    cbn [map In] in Absent.
    rewrite (IH candidate after key Prepared) by tauto.
    eapply import_staging_changes_only_the_received_key; eauto.
Qed.

Theorem import_history_overlay_rejects_conflicting_duplicate_rows :
  forall Hash before rows key first second,
  In (key, first) rows -> In (key, second) rows -> first <> second ->
  forall after, import_prepare_history_overlay Hash before rows <> ImportHistoryOverlayReady after.
Proof.
  intros Hash before rows key first second First Second Different after Prepared.
  apply import_history_overlay_authenticates_every_received_row in Prepared.
  rewrite Forall_forall in Prepared.
  pose proof (Prepared (key, first) First) as [_ Left].
  pose proof (Prepared (key, second) Second) as [_ Right].
  cbn [fst snd] in Left, Right. congruence.
Qed.

Theorem import_accepted_history_row_order_does_not_change_the_overlay :
  forall Hash before first second left right,
  Permutation first second ->
  import_prepare_history_overlay Hash before first = ImportHistoryOverlayReady left ->
  import_prepare_history_overlay Hash before second = ImportHistoryOverlayReady right ->
  forall key, left key = right key.
Proof.
  intros Hash before first second left right Permuted Left Right key.
  destruct (in_dec (list_eq_dec Nat.eq_dec) key (map fst first)) as [Present|Absent].
  - apply in_map_iff in Present as [[row_key bytes] [Same Present]]. cbn [fst] in Same. subst row_key.
    pose proof (import_history_overlay_authenticates_every_received_row _ _ _ _ Left) as LeftRows.
    pose proof (import_history_overlay_authenticates_every_received_row _ _ _ _ Right) as RightRows.
    rewrite Forall_forall in LeftRows, RightRows.
    destruct (LeftRows (key, bytes) Present) as [_ LeftRead].
    assert (In (key, bytes) second) as Other by (eapply Permutation_in; eauto).
    destruct (RightRows (key, bytes) Other) as [_ RightRead].
    cbn [fst snd] in LeftRead, RightRead. now rewrite LeftRead, RightRead.
  - rewrite (import_history_overlay_does_not_change_unreceived_keys _ _ _ _ _ Left Absent).
    symmetry. apply (import_history_overlay_does_not_change_unreceived_keys _ _ _ _ _ Right).
    intros Present. apply Absent.
    eapply Permutation_in; [apply Permutation_sym, Permutation_map; exact Permuted|exact Present].
Qed.

Definition import_history_rows_compatible (Hash : list nat -> list nat)
    (before : ImportFallibleHistory) (rows : list ImportReceivedHistoryRow) : Prop :=
  Forall (fun row =>
    (exists edges, import_check_received_history_row Hash (fst row) (snd row) = Some edges) /\
    (before (fst row) = ImportRootLookupAbsent \/
      before (fst row) = ImportRootLookupFound (snd row))) rows.

Definition import_history_rows_consistent (rows : list ImportReceivedHistoryRow) : Prop :=
  forall key first second, In (key, first) rows -> In (key, second) rows -> first = second.

Theorem import_staging_preserves_nonabsent_observations : forall Hash before row after key,
  import_stage_history_row Hash before row = ImportHistoryOverlayReady after ->
  before key <> ImportRootLookupAbsent -> after key = before key.
Proof.
  intros Hash before [row_key bytes] after key Staged Present.
  unfold import_stage_history_row in Staged.
  destruct (import_check_received_history_row Hash row_key bytes); [|discriminate].
  destruct (before row_key) as [|stored|] eqn:Old; [| |discriminate].
  - inversion Staged; subst after. unfold import_candidate_history_insert.
    destruct (list_eq_dec Nat.eq_dec key row_key) as [Same|Different]; [|reflexivity].
    subst key. contradiction.
  - destruct (list_eq_dec Nat.eq_dec stored bytes); [|discriminate].
    inversion Staged; subst after. reflexivity.
Qed.

Theorem import_history_overlay_preserves_nonabsent_observations : forall Hash rows before after key,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady after ->
  before key <> ImportRootLookupAbsent -> after key = before key.
Proof.
  intros Hash rows. induction rows as [|row rest IH]; intros before after key Prepared Present.
  - inversion Prepared; subst after. reflexivity.
  - cbn [import_prepare_history_overlay] in Prepared.
    destruct (import_stage_history_row Hash before row) as [candidate|invalid|conflict|failed] eqn:Staged;
      try discriminate.
    pose proof (import_staging_preserves_nonabsent_observations _ _ _ _ _ Staged Present) as Same.
    rewrite (IH candidate after key Prepared) by congruence. exact Same.
Qed.

Theorem import_prepared_history_rows_are_initially_compatible_and_consistent :
  forall Hash before rows after,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady after ->
  import_history_rows_compatible Hash before rows /\ import_history_rows_consistent rows.
Proof.
  intros Hash before rows after Prepared.
  pose proof (import_history_overlay_authenticates_every_received_row _ _ _ _ Prepared) as Rows.
  split.
  - unfold import_history_rows_compatible. eapply Forall_impl; [|exact Rows].
    intros [key bytes] [Checked Found]. cbn [fst snd] in *.
    split; [exact Checked|]. destruct (before key) as [|stored|] eqn:Old.
    + left. reflexivity.
    + right. assert (before key <> ImportRootLookupAbsent) as Present by congruence.
      pose proof (import_history_overlay_preserves_nonabsent_observations _ _ _ _ _ Prepared Present) as Same.
      congruence.
    + assert (before key <> ImportRootLookupAbsent) as Present by congruence.
      pose proof (import_history_overlay_preserves_nonabsent_observations _ _ _ _ _ Prepared Present) as Same.
      congruence.
  - intros key first second First Second. rewrite Forall_forall in Rows.
    destruct (Rows (key, first) First) as [_ FirstRead].
    destruct (Rows (key, second) Second) as [_ SecondRead]. cbn [fst snd] in *. congruence.
Qed.

Theorem import_staging_preserves_remaining_row_compatibility :
  forall Hash before key bytes rest candidate,
  import_history_rows_compatible Hash before rest ->
  import_history_rows_consistent ((key, bytes) :: rest) ->
  import_stage_history_row Hash before (key, bytes) = ImportHistoryOverlayReady candidate ->
  import_history_rows_compatible Hash candidate rest.
Proof.
  intros Hash before key bytes rest candidate Compatible Consistent Staged.
  unfold import_history_rows_compatible in *. rewrite Forall_forall in *.
  intros [other data] Present. destruct (Compatible (other, data) Present) as [Checked Old].
  split; [exact Checked|]. cbn [fst snd] in *.
  destruct (list_eq_dec Nat.eq_dec other key) as [Same|Different].
  - subst other. assert (bytes = data) as Equal by (eapply Consistent; [left; reflexivity|right; exact Present]).
    right. subst data.
    exact (proj2 (import_staged_history_row_has_authenticated_binding _ _ _ _ _ Staged)).
  - rewrite (import_staging_changes_only_the_received_key _ _ _ _ other Staged) by exact Different.
    exact Old.
Qed.

Theorem import_valid_compatible_consistent_history_rows_can_be_prepared :
  forall Hash rows before,
  import_history_rows_compatible Hash before rows -> import_history_rows_consistent rows ->
  exists after, import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady after.
Proof.
  intros Hash rows. induction rows as [|[key bytes] rest IH]; intros before Compatible Consistent.
  - exists before. reflexivity.
  - inversion Compatible as [|row tail [[edges Checked] Old] Tail]; subst row tail.
    cbn [fst snd] in Checked, Old.
    destruct (import_valid_compatible_history_row_can_be_staged _ _ _ _ _ Checked Old)
      as [candidate Staged].
    assert (import_history_rows_compatible Hash candidate rest) as Remaining.
    { eapply import_staging_preserves_remaining_row_compatibility; eauto. }
    assert (import_history_rows_consistent rest) as ConsistentRest.
    { intros query first second First Second. eapply Consistent; right; eauto. }
    destruct (IH candidate Remaining ConsistentRest) as [after Prepared].
    exists after. cbn [import_prepare_history_overlay]. now rewrite Staged.
Qed.

Theorem import_valid_history_rows_accept_every_permutation :
  forall Hash before rows permuted,
  import_history_rows_compatible Hash before rows -> import_history_rows_consistent rows ->
  Permutation rows permuted ->
  exists first second,
    import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady first /\
    import_prepare_history_overlay Hash before permuted = ImportHistoryOverlayReady second /\
    forall key, first key = second key.
Proof.
  intros Hash before rows permuted Compatible Consistent Permuted.
  assert (import_history_rows_compatible Hash before permuted) as OtherCompatible.
  { unfold import_history_rows_compatible in *. rewrite Forall_forall in *.
    intros row Present. apply Compatible. eapply Permutation_in; [apply Permutation_sym; exact Permuted|exact Present]. }
  assert (import_history_rows_consistent permuted) as OtherConsistent.
  { intros key first second First Second. apply (Consistent key first second).
    - eapply Permutation_in; [apply Permutation_sym; exact Permuted|exact First].
    - eapply Permutation_in; [apply Permutation_sym; exact Permuted|exact Second]. }
  destruct (import_valid_compatible_consistent_history_rows_can_be_prepared _ _ _ Compatible Consistent)
    as [first First].
  destruct (import_valid_compatible_consistent_history_rows_can_be_prepared _ _ _ OtherCompatible OtherConsistent)
    as [second Second].
  exists first, second. split; [exact First|]. split; [exact Second|].
  eapply import_accepted_history_row_order_does_not_change_the_overlay; eauto.
Qed.

Theorem import_successful_history_order_implies_every_permutation_succeeds :
  forall Hash before rows permuted after,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady after ->
  Permutation rows permuted ->
  exists other,
    import_prepare_history_overlay Hash before permuted = ImportHistoryOverlayReady other /\
    forall key, other key = after key.
Proof.
  intros Hash before rows permuted after Prepared Permuted.
  destruct (import_prepared_history_rows_are_initially_compatible_and_consistent _ _ _ _ Prepared)
    as [Compatible Consistent].
  destruct (import_valid_history_rows_accept_every_permutation _ _ _ _ Compatible Consistent Permuted)
    as [first [other [First [Other Same]]]].
  rewrite Prepared in First. inversion First; subst first.
  exists other. split; [exact Other|]. intro key. symmetry. apply Same.
Qed.

Definition import_received_keyset_equalb (received expected : list nat) : bool :=
  forallb (fun key => if in_dec Nat.eq_dec key expected then true else false) received &&
  forallb (fun key => if in_dec Nat.eq_dec key received then true else false) expected.

Theorem import_received_keyset_equality_is_exact : forall received expected,
  import_received_keyset_equalb received expected = true <->
    forall key, In key received <-> In key expected.
Proof.
  intros received expected. unfold import_received_keyset_equalb.
  rewrite andb_true_iff, !forallb_forall. split.
  - intros [Forward Backward] key. split; intros Present.
    + specialize (Forward key Present). destruct (in_dec Nat.eq_dec key expected); congruence.
    + specialize (Backward key Present). destruct (in_dec Nat.eq_dec key received); congruence.
  - intros Same. split; intros key Present.
    + destruct (in_dec Nat.eq_dec key expected); [reflexivity|]. exfalso. apply n. now apply Same.
    + destruct (in_dec Nat.eq_dec key received); [reflexivity|]. exfalso. apply n. now apply Same.
Qed.

Definition import_reply_path_equalb (first second : ImportWirePath) : bool :=
  if import_cursor_path_eq_dec first second then true else false.

Theorem import_reply_path_comparison_is_exact : forall first second,
  import_reply_path_equalb first second = true <-> first = second.
Proof.
  intros. unfold import_reply_path_equalb.
  destruct (import_cursor_path_eq_dec first second); split; congruence.
Qed.

Definition import_check_reply_metadata (request received_start received_next : ImportWirePath)
    (received_history received_cold : list nat) (computed : ImportWireReferences) : bool :=
  import_reply_path_equalb received_start request &&
  import_reply_path_equalb received_next (import_reply_next_path computed) &&
  import_received_keyset_equalb received_history (import_reply_history computed) &&
  import_received_keyset_equalb received_cold (import_reply_leaves computed).

Theorem import_accepted_reply_metadata_has_exact_coverage :
  forall request start next history cold computed,
  import_check_reply_metadata request start next history cold computed = true <->
  start = request /\ next = import_reply_next_path computed /\
    (forall key, In key history <-> In key (import_reply_history computed)) /\
    (forall key, In key cold <-> In key (import_reply_leaves computed)).
Proof.
  intros. unfold import_check_reply_metadata.
  rewrite !andb_true_iff, !import_reply_path_comparison_is_exact,
    !import_received_keyset_equality_is_exact. tauto.
Qed.

Theorem import_accepted_reply_matches_checked_export :
  forall input result computed request start next history cold,
  import_adapt_wire_result input result = ImportWireSuccess computed ->
  import_check_reply_metadata request start next history cold computed = true ->
  exists entries frames,
    result = ImportEvaluationSuccess entries frames /\ start = request /\
    import_cursor_encode (import_wire_next_cursor input entries frames) = Some next /\
    (forall key, In key history <-> In key (import_export_history_keys entries)) /\
    (forall key, In key cold <-> In key (import_export_leaf_keys entries)).
Proof.
  intros input result computed request start next history cold Adapted Accepted.
  apply import_accepted_reply_metadata_has_exact_coverage in Accepted as [Start [Next [History Cold]]].
  destruct (import_wire_success_preserves_exact_occurrences _ _ _ Adapted)
    as [entries [frames [Result [HistoryKeys [ColdKeys [_ Encoded]]]]]].
  exists entries, frames. subst start next. rewrite HistoryKeys in History. rewrite ColdKeys in Cold.
  auto.
Qed.

Theorem import_missing_or_extra_received_key_rejects :
  forall request start next history cold computed key,
  ((In key (import_reply_history computed) /\ ~ In key history) \/
   (In key history /\ ~ In key (import_reply_history computed)) \/
   (In key (import_reply_leaves computed) /\ ~ In key cold) \/
   (In key cold /\ ~ In key (import_reply_leaves computed))) ->
  import_check_reply_metadata request start next history cold computed <> true.
Proof.
  intros request start next history cold computed key Different Accepted.
  apply import_accepted_reply_metadata_has_exact_coverage in Accepted as [_ [_ [History Cold]]].
  specialize (History key). specialize (Cold key). tauto.
Qed.

Theorem import_forged_continuation_rejects : forall request start next history cold computed,
  next <> import_reply_next_path computed ->
  import_check_reply_metadata request start next history cold computed <> true.
Proof.
  intros request start next history cold computed Different Accepted.
  apply import_accepted_reply_metadata_has_exact_coverage in Accepted as [_ [Same _]]. contradiction.
Qed.

Theorem import_wrong_requested_path_rejects : forall request start next history cold computed,
  start <> request -> import_check_reply_metadata request start next history cold computed <> true.
Proof.
  intros request start next history cold computed Different Accepted.
  apply import_accepted_reply_metadata_has_exact_coverage in Accepted as [Same _]. contradiction.
Qed.

Inductive ImportCheckedHistoryLookup : Type :=
| ImportCheckedHistoryFound (edges : list ImportWireEdge)
| ImportCheckedHistoryAbsent
| ImportCheckedHistoryInvalid
| ImportCheckedHistoryStorageFailure.

Definition import_check_overlay_history_lookup (Hash : list nat -> list nat)
    (candidate : ImportFallibleHistory) (key : list nat) : ImportCheckedHistoryLookup :=
  match candidate key with
  | ImportRootLookupAbsent => ImportCheckedHistoryAbsent
  | ImportRootLookupFailure => ImportCheckedHistoryStorageFailure
  | ImportRootLookupFound bytes => match import_check_received_history_row Hash key bytes with
    | Some edges => ImportCheckedHistoryFound edges
    | None => ImportCheckedHistoryInvalid
    end
  end.

Definition import_overlay_traversal_reader (Hash : list nat -> list nat)
    (candidate : ImportFallibleHistory) : ImportHistoryReader :=
  fun key => match import_check_overlay_history_lookup Hash candidate key with
    | ImportCheckedHistoryFound edges => Some edges
    | _ => None
    end.

Theorem import_overlay_traversal_has_checked_byte_evidence : forall Hash candidate key edges,
  import_overlay_traversal_reader Hash candidate key = Some edges ->
  exists bytes, candidate key = ImportRootLookupFound bytes /\
    length key = 32 /\ Forall import_wire_byte key /\ Hash bytes = key /\
    import_parse_wire_node bytes = Some edges.
Proof.
  intros Hash candidate key edges Read.
  unfold import_overlay_traversal_reader, import_check_overlay_history_lookup in Read.
  destruct (candidate key) as [|bytes|] eqn:Found; try discriminate.
  destruct (import_check_received_history_row Hash key bytes) as [parsed|] eqn:Checked; try discriminate.
  inversion Read; subst parsed. exists bytes. split; auto.
  now apply import_received_history_check_authenticates_bytes in Checked.
Qed.

Theorem import_checked_overlay_reads_survive_compatible_observations : forall Hash first last,
  import_history_observation_extension first last ->
  import_history_reader_extension (import_overlay_traversal_reader Hash first)
    (import_overlay_traversal_reader Hash last).
Proof.
  intros Hash first last Extension key edges Read.
  unfold import_overlay_traversal_reader, import_check_overlay_history_lookup in *.
  destruct (first key) as [|bytes|] eqn:Found; try discriminate.
  now rewrite (Extension key bytes Found).
Qed.

Theorem import_overlay_preparation_preserves_successful_traversal_reads : forall Hash before rows after,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady after ->
  import_history_reader_extension (import_overlay_traversal_reader Hash before)
    (import_overlay_traversal_reader Hash after).
Proof.
  intros. apply import_checked_overlay_reads_survive_compatible_observations.
  eapply import_history_overlay_preserves_every_observed_binding; eauto.
Qed.

Theorem import_prepared_history_rows_are_readable_by_checked_traversal : forall Hash before rows after,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady after ->
  Forall (fun row => exists edges,
    import_overlay_traversal_reader Hash after (fst row) = Some edges /\
    import_parse_wire_node (snd row) = Some edges) rows.
Proof.
  intros Hash before rows after Prepared.
  apply import_history_overlay_authenticates_every_received_row in Prepared.
  induction Prepared as [|[key bytes] rest [[edges Checked] Found] Tail IH]; constructor; auto.
  exists edges. split.
  - unfold import_overlay_traversal_reader, import_check_overlay_history_lookup.
    cbn [fst snd] in *. now rewrite Found, Checked.
  - now apply import_received_history_check_authenticates_bytes in Checked as [_ [_ [_ Parsed]]].
Qed.

Theorem import_prepared_interleaved_reply_has_canonical_coverage :
  forall Hash before rows candidate last input skip budget entries final computed request start next cold,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady candidate ->
  import_interleaved_cursor_export (import_overlay_traversal_reader Hash candidate)
    (import_overlay_traversal_reader Hash last) input skip budget entries final ->
  import_cursor_encode input = Some request ->
  import_adapt_wire_result input (ImportEvaluationSuccess entries final) = ImportWireSuccess computed ->
  import_check_reply_metadata request start next (map (fun row => import_key_to_nat (fst row)) rows)
    cold computed = true ->
  import_export_from_cursor (import_overlay_traversal_reader Hash last) input skip budget entries final /\
    length (import_export_history_keys entries) <= budget /\ start = request /\
    import_cursor_decode start = Some input /\
    import_cursor_encode (import_wire_next_cursor input entries final) = Some next /\
    (forall key, In key (map (fun row => import_key_to_nat (fst row)) rows) <->
      In key (import_export_history_keys entries)) /\
    (forall key, In key cold <-> In key (import_export_leaf_keys entries)) /\
    Forall (fun row => exists edges,
      length (fst row) = 32 /\ Forall import_wire_byte (fst row) /\
      Hash (snd row) = fst row /\ import_parse_wire_node (snd row) = Some edges) rows /\
    (forall key edges, import_overlay_traversal_reader Hash last key = Some edges ->
      exists bytes, last key = ImportRootLookupFound bytes /\
        length key = 32 /\ Forall import_wire_byte key /\ Hash bytes = key /\
        import_parse_wire_node bytes = Some edges).
Proof.
  intros Hash before rows candidate last input skip budget entries final computed request start next cold
    Prepared Export Request Adapted Accepted.
  pose proof (import_interleaved_cursor_export_has_exact_final_view _ _ _ _ _ _ _ Export) as FinalExport.
  pose proof (import_cursor_export_respects_history_budget _ _ _ _ _ _ FinalExport) as Budget.
  apply import_accepted_reply_metadata_has_exact_coverage in Accepted as [Start [Next [History Cold]]].
  destruct (import_wire_success_preserves_exact_occurrences _ _ _ Adapted)
    as [actual [frames [Result [HistoryKeys [ColdKeys [_ Encoded]]]]]].
  inversion Result; subst actual frames. subst start next.
  rewrite HistoryKeys in History. rewrite ColdKeys in Cold.
  split; [exact FinalExport|]. split; [exact Budget|]. split; [reflexivity|].
  split; [now apply import_cursor_decode_encode_round_trip|].
  split; [exact Encoded|]. split; [exact History|]. split; [exact Cold|].
  split.
  - apply import_history_overlay_authenticates_every_received_row in Prepared.
    eapply Forall_impl; [|exact Prepared]. intros row [[edges Checked] _].
    exists edges. now apply import_received_history_check_authenticates_bytes in Checked.
  - intros. eapply import_overlay_traversal_has_checked_byte_evidence; eauto.
Qed.

Theorem import_checked_cursor_reply_preserves_exact_occurrences_and_singleton_origin :
  forall Hash before rows candidate last input skip budget entries final computed request start next cold
    original origin,
  import_prepare_history_overlay Hash before rows = ImportHistoryOverlayReady candidate ->
  import_interleaved_cursor_export (import_overlay_traversal_reader Hash candidate)
    (import_overlay_traversal_reader Hash last) input skip budget entries final ->
  import_cursor_encode input = Some request ->
  import_adapt_wire_result input (ImportEvaluationSuccess entries final) = ImportWireSuccess computed ->
  import_check_reply_metadata request start next (map (fun row => import_key_to_nat (fst row)) rows)
    cold computed = true ->
  import_history_path (import_overlay_traversal_reader Hash candidate) original origin (import_cursor_root input) ->
  exists located,
    import_located_cursor_export (import_overlay_traversal_reader Hash last) origin input skip budget located final /\
    map import_located_entry located = entries /\
    Forall (import_complete_occurrence_witness (import_overlay_traversal_reader Hash last) original) located /\
    import_cursor_encode (import_wire_next_cursor input (map import_located_entry located) final) = Some next /\
    (forall selected, final = [] -> import_last_located_history located = Some selected ->
      exists raw_key, import_cursor_encode (ImportCursorStart raw_key) = Some next /\
        import_history_path (import_overlay_traversal_reader Hash last) original (import_located_path selected) raw_key).
Proof.
  intros Hash before rows candidate last input skip budget entries final computed request start next cold
    original origin Prepared Run Request Adapted Accepted Origin.
  destruct (import_prepared_interleaved_reply_has_canonical_coverage _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
    Prepared Run Request Adapted Accepted)
    as [Export [_ [_ [_ [Next [_ [_ [_ Reads]]]]]]]].
  assert (import_history_path (import_overlay_traversal_reader Hash last) original origin
    (import_cursor_root input)) as FinalOrigin.
  { eapply import_history_path_survives_compatible_writes; [|exact Origin].
    eapply import_interleaved_cursor_export_preserves_starting_reads; eauto. }
  destruct (import_cursor_export_has_complete_occurrence_annotations _ _ _ _ _ _ _ _ Export FinalOrigin)
    as [located [Located [Erase Complete]]].
  exists located. split; [exact Located|]. split; [exact Erase|]. split; [exact Complete|].
  split; [now rewrite Erase|]. intros selected Empty Last. subst final.
  assert (forall key edges, import_overlay_traversal_reader Hash last key = Some edges ->
    import_cursor_word_validb key = true) as Valid.
  { intros key edges Read. destruct (Reads key edges Read) as [bytes [_ [Width [Domain _]]]].
    apply import_cursor_word_validity_characterization. auto. }
  destruct (import_exhausted_cursor_keeps_exact_last_occurrence_origin _ input _ _ _ Complete Last Valid)
    as [raw_key [Cursor Path]].
  exists raw_key. split; [|exact Path]. rewrite <- Cursor, Erase. exact Next.
Qed.
