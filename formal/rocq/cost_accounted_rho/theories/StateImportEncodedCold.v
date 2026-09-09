From Stdlib Require Import List Arith Bool.
From CostAccountedRho Require Import StateImportClosure StateImportStorage StateImportCodec StateImportCold.
Import ListNotations.

Definition ImportEncodedColdStore : Type := ImportColdAlias -> nat -> option (list nat).

Definition import_cold_alias_eq_dec : forall first second : ImportColdAlias,
  {first = second} + {first <> second}.
Proof. decide equality. Defined.

Definition import_decode_encoded_alias (bytes : option (list nat)) : option (option ImportLeaf) :=
  match bytes with
  | None => Some None
  | Some input => option_map (@Some ImportLeaf) (import_read_persisted_leaf input)
  end.

Definition import_encoded_raw_first_read (first second : ImportEncodedColdStore) (key : nat)
    : option (option ImportLeaf) :=
  match first ImportRawAlias key with
  | Some input => import_decode_encoded_alias (Some input)
  | None => import_decode_encoded_alias (second ImportLegacyAlias key)
  end.

Definition import_encoded_alias_agrees (current : option (list nat)) (leaf : ImportLeaf) : bool :=
  match current with
  | None => true
  | Some input => match import_read_persisted_leaf input with
    | None => false
    | Some found => if import_leaf_eq_dec found leaf then true else false
    end
  end.

Theorem import_encoded_alias_guard_preserves_malformed_presence : forall current leaf,
  import_encoded_alias_agrees current leaf = true <->
  match current with None => True | Some input => import_read_persisted_leaf input = Some leaf end.
Proof.
  intros current leaf. destruct current as [input|]; cbn [import_encoded_alias_agrees]; [|tauto].
  destruct (import_read_persisted_leaf input) as [found|]; [|split; discriminate].
  destruct (import_leaf_eq_dec found leaf); split; intros; congruence.
Qed.

Definition import_resolve_encoded_cold (store : ImportEncodedColdStore) (key : nat)
    : option (option ImportLeaf) :=
  match import_encoded_raw_first_read store store key with
  | Some (Some leaf) =>
      if import_encoded_alias_agrees (store ImportRawAlias key) leaf &&
         import_encoded_alias_agrees (store ImportLegacyAlias key) leaf
      then Some (Some leaf) else None
  | other => other
  end.

Theorem import_encoded_strict_resolution_requires_actual_reader_and_both_guards :
  forall store key leaf,
  import_resolve_encoded_cold store key = Some (Some leaf) <->
  import_encoded_raw_first_read store store key = Some (Some leaf) /\
  import_encoded_alias_agrees (store ImportRawAlias key) leaf = true /\
  import_encoded_alias_agrees (store ImportLegacyAlias key) leaf = true.
Proof.
  intros store key leaf. unfold import_resolve_encoded_cold. split.
  - destruct (import_encoded_raw_first_read store store key) as [[found|]|] eqn:Read;
      try discriminate.
    destruct (_ && _) eqn:Guards; [|discriminate]. intros Accepted.
    inversion Accepted; subst found. apply andb_true_iff in Guards. auto.
  - intros [Read [Raw Legacy]]. now rewrite Read, Raw, Legacy.
Qed.

Theorem import_encoded_success_has_present_decodable_bytes : forall first second key leaf,
  import_encoded_raw_first_read first second key = Some (Some leaf) ->
  (exists bytes, first ImportRawAlias key = Some bytes /\ import_read_persisted_leaf bytes = Some leaf) \/
  (first ImportRawAlias key = None /\ exists bytes,
    second ImportLegacyAlias key = Some bytes /\ import_read_persisted_leaf bytes = Some leaf).
Proof.
  intros first second key leaf Read. unfold import_encoded_raw_first_read in Read.
  destruct (first ImportRawAlias key) as [raw|] eqn:Raw.
  - cbn [import_decode_encoded_alias] in Read.
    destruct (import_read_persisted_leaf raw) as [found|] eqn:Parsed; [|discriminate].
    inversion Read; subst found. left. exists raw. auto.
  - destruct (second ImportLegacyAlias key) as [legacy|] eqn:Legacy; [|discriminate].
    cbn [import_decode_encoded_alias] in Read.
    destruct (import_read_persisted_leaf legacy) as [found|] eqn:Parsed; [|discriminate].
    inversion Read; subst found. right. split; auto. exists legacy. auto.
Qed.

Theorem import_encoded_guards_and_presence_establish_strict_resolution : forall store key leaf,
  import_encoded_alias_agrees (store ImportRawAlias key) leaf = true ->
  import_encoded_alias_agrees (store ImportLegacyAlias key) leaf = true ->
  (exists alias bytes, store alias key = Some bytes) ->
  import_resolve_encoded_cold store key = Some (Some leaf).
Proof.
  intros store key leaf Raw Legacy Present.
  apply import_encoded_strict_resolution_requires_actual_reader_and_both_guards.
  split; [|auto]. unfold import_encoded_raw_first_read.
  apply import_encoded_alias_guard_preserves_malformed_presence in Raw.
  apply import_encoded_alias_guard_preserves_malformed_presence in Legacy.
  destruct (store ImportRawAlias key) as [raw|] eqn:RawBytes.
  - cbn [import_decode_encoded_alias]. now rewrite Raw.
  - destruct (store ImportLegacyAlias key) as [legacy|] eqn:LegacyBytes.
    + cbn [import_decode_encoded_alias]. now rewrite Legacy.
    + destruct Present as [alias [bytes Found]]. destruct alias; congruence.
Qed.

Definition import_fill_encoded_alias (store : ImportEncodedColdStore)
    (alias : ImportColdAlias) (key : nat) (input : list nat) : ImportEncodedColdStore :=
  fun selected query =>
    if import_cold_alias_eq_dec selected alias then
      if Nat.eq_dec query key then
        match store selected query with Some previous => Some previous | None => Some input end
      else store selected query
    else store selected query.

Definition import_encoded_extension (first last : ImportEncodedColdStore) : Prop :=
  forall alias key bytes, first alias key = Some bytes -> last alias key = Some bytes.

Theorem import_fill_alias_never_replaces_existing_bytes : forall store alias key input,
  import_encoded_extension store (import_fill_encoded_alias store alias key input).
Proof.
  intros store alias key input selected query bytes Existing.
  unfold import_fill_encoded_alias.
  destruct (import_cold_alias_eq_dec selected alias); [destruct (Nat.eq_dec query key)|];
    now rewrite Existing.
Qed.

Theorem import_fill_alias_preserves_other_keys : forall store alias key input selected query,
  query <> key -> import_fill_encoded_alias store alias key input selected query = store selected query.
Proof.
  intros store alias key input selected query Different. unfold import_fill_encoded_alias.
  destruct (import_cold_alias_eq_dec selected alias); [destruct (Nat.eq_dec query key)|]; congruence.
Qed.

Theorem import_fill_alias_has_target_bytes : forall store alias key input,
  exists bytes, import_fill_encoded_alias store alias key input alias key = Some bytes.
Proof.
  intros store alias key input. unfold import_fill_encoded_alias.
  destruct (import_cold_alias_eq_dec alias alias); [|contradiction].
  destruct (Nat.eq_dec key key); [|contradiction].
  destruct (store alias key); eauto.
Qed.

Theorem import_fill_alias_preserves_candidate_agreement : forall store alias key input selected leaf,
  import_read_persisted_leaf input = Some leaf ->
  import_encoded_alias_agrees (store selected key) leaf = true ->
  import_encoded_alias_agrees (import_fill_encoded_alias store alias key input selected key) leaf = true.
Proof.
  intros store alias key input selected leaf Parsed Agrees. unfold import_fill_encoded_alias.
  destruct (import_cold_alias_eq_dec selected alias); auto.
  destruct (Nat.eq_dec key key); [|contradiction].
  destruct (store selected key) as [old|]; auto.
  apply import_encoded_alias_guard_preserves_malformed_presence. exact Parsed.
Qed.

Definition import_guarded_encoded_insert (store : ImportEncodedColdStore)
    (alias : ImportColdAlias) (key : nat) (input : list nat) : option ImportEncodedColdStore :=
  match import_read_persisted_leaf input with
  | None => None
  | Some leaf =>
      if import_encoded_alias_agrees (store ImportRawAlias key) leaf &&
         import_encoded_alias_agrees (store ImportLegacyAlias key) leaf
      then Some (import_fill_encoded_alias store alias key input) else None
  end.

Theorem import_guarded_encoded_insert_has_checked_commit_inputs : forall store alias key input result,
  import_guarded_encoded_insert store alias key input = Some result ->
  exists leaf, import_read_persisted_leaf input = Some leaf /\
    import_encoded_alias_agrees (store ImportRawAlias key) leaf = true /\
    import_encoded_alias_agrees (store ImportLegacyAlias key) leaf = true /\
    result = import_fill_encoded_alias store alias key input.
Proof.
  intros store alias key input result Inserted. unfold import_guarded_encoded_insert in Inserted.
  destruct (import_read_persisted_leaf input) as [leaf|] eqn:Parsed; [|discriminate].
  destruct (_ && _) eqn:Guards; [|discriminate]. inversion Inserted; subst result.
  apply andb_true_iff in Guards. exists leaf. tauto.
Qed.

Theorem import_guarded_encoded_insert_preserves_bytes_and_resolves_the_inserted_leaf :
  forall store alias key input result leaf,
  import_read_persisted_leaf input = Some leaf ->
  import_guarded_encoded_insert store alias key input = Some result ->
  import_encoded_extension store result /\ import_resolve_encoded_cold result key = Some (Some leaf).
Proof.
  intros store alias key input result leaf Parsed Inserted.
  destruct (import_guarded_encoded_insert_has_checked_commit_inputs _ _ _ _ _ Inserted)
    as [found [Found [Raw [Legacy Same]]]].
  rewrite Parsed in Found. inversion Found; subst found result. split.
  - apply import_fill_alias_never_replaces_existing_bytes.
  - apply import_encoded_guards_and_presence_establish_strict_resolution.
    + now apply import_fill_alias_preserves_candidate_agreement.
    + now apply import_fill_alias_preserves_candidate_agreement.
    + destruct (import_fill_alias_has_target_bytes store alias key input) as [bytes Present].
      exists alias, bytes. exact Present.
Qed.

Theorem import_existing_resolved_leaf_identifies_every_accepted_candidate : forall store key old leaf,
  import_resolve_encoded_cold store key = Some (Some old) ->
  import_encoded_alias_agrees (store ImportRawAlias key) leaf = true ->
  import_encoded_alias_agrees (store ImportLegacyAlias key) leaf = true -> old = leaf.
Proof.
  intros store key old leaf Resolved Raw Legacy.
  apply import_encoded_strict_resolution_requires_actual_reader_and_both_guards in Resolved as [Read _].
  apply import_encoded_success_has_present_decodable_bytes in Read.
  destruct Read as [[bytes [Found Parsed]]|[_ [bytes [Found Parsed]]]].
  - rewrite Found in Raw. apply import_encoded_alias_guard_preserves_malformed_presence in Raw.
    cbn in Raw. congruence.
  - rewrite Found in Legacy. apply import_encoded_alias_guard_preserves_malformed_presence in Legacy.
    cbn in Legacy. congruence.
Qed.

Theorem import_guarded_encoded_insert_preserves_every_resolved_leaf :
  forall store alias key input result query leaf,
  import_guarded_encoded_insert store alias key input = Some result ->
  import_resolve_encoded_cold store query = Some (Some leaf) ->
  import_resolve_encoded_cold result query = Some (Some leaf).
Proof.
  intros store alias key input result query leaf Inserted Resolved.
  destruct (import_guarded_encoded_insert_has_checked_commit_inputs _ _ _ _ _ Inserted)
    as [incoming [Parsed [Raw [Legacy Same]]]].
  destruct (Nat.eq_dec query key) as [Equal|Different].
  - subst query.
    pose proof (import_existing_resolved_leaf_identifies_every_accepted_candidate
      _ _ _ _ Resolved Raw Legacy) as Equal. subst incoming.
    eapply import_guarded_encoded_insert_preserves_bytes_and_resolves_the_inserted_leaf in Inserted;
      eauto. tauto.
  - subst result. unfold import_resolve_encoded_cold, import_encoded_raw_first_read.
    rewrite !import_fill_alias_preserves_other_keys by assumption.
    exact Resolved.
Qed.

Definition import_encoded_logical_view (history : nat -> option (list ImportRadixEdge))
    (store : ImportEncodedColdStore) : ImportStore :=
  {| import_history := history;
     import_cold_raw := fun key => match import_resolve_encoded_cold store key with
       | Some decoded => decoded | None => None end;
     import_cold_legacy := fun _ => None |}.

Theorem import_encoded_logical_resolution_matches_only_strict_success : forall history store key leaf,
  import_resolve_cold (import_encoded_logical_view history store) key = Some leaf <->
    import_resolve_encoded_cold store key = Some (Some leaf).
Proof.
  intros history store key leaf.
  unfold import_resolve_cold, import_encoded_logical_view. cbn.
  destruct (import_resolve_encoded_cold store key) as [[found|]|]; split; intros; congruence.
Qed.

Theorem import_guarded_encoded_insert_refines_logical_binding_extension :
  forall history store alias key input result,
  import_guarded_encoded_insert store alias key input = Some result ->
  import_binding_extension (import_encoded_logical_view history store)
    (import_encoded_logical_view history result).
Proof.
  intros history store alias key input result Inserted. split.
  - intros query children Found. exact Found.
  - intros query leaf Found.
    apply import_encoded_logical_resolution_matches_only_strict_success in Found.
    apply import_encoded_logical_resolution_matches_only_strict_success.
    eapply import_guarded_encoded_insert_preserves_every_resolved_leaf; eauto.
Qed.

Definition import_encoded_attempt (store : ImportEncodedColdStore) (alias : ImportColdAlias)
    (key : nat) (input : list nat) (storage_ok : bool) : ImportEncodedColdStore * bool :=
  match import_guarded_encoded_insert store alias key input with
  | None => (store, false)
  | Some candidate => if storage_ok then (candidate, true) else (store, false)
  end.

Theorem import_failed_encoded_attempt_preserves_the_entire_byte_store :
  forall store alias key input storage_ok result,
  import_encoded_attempt store alias key input storage_ok = (result, false) -> result = store.
Proof.
  intros store alias key input storage_ok result Attempt. unfold import_encoded_attempt in Attempt.
  destruct (import_guarded_encoded_insert store alias key input);
    [destruct storage_ok|]; congruence.
Qed.

Theorem import_encoded_attempt_preserves_physical_bytes_and_logical_bindings :
  forall history store alias key input storage_ok result success,
  import_encoded_attempt store alias key input storage_ok = (result, success) ->
  import_encoded_extension store result /\
  import_binding_extension (import_encoded_logical_view history store)
    (import_encoded_logical_view history result).
Proof.
  intros history store alias key input storage_ok result success Attempt.
  destruct success.
  - unfold import_encoded_attempt in Attempt.
    destruct (import_guarded_encoded_insert store alias key input) as [candidate|] eqn:Inserted;
      [|discriminate]. destruct storage_ok; [|discriminate]. inversion Attempt; subst candidate.
    split.
    + destruct (import_guarded_encoded_insert_has_checked_commit_inputs _ _ _ _ _ Inserted)
        as [leaf [Parsed _]].
      eapply import_guarded_encoded_insert_preserves_bytes_and_resolves_the_inserted_leaf in Inserted;
        eauto. tauto.
    + eapply import_guarded_encoded_insert_refines_logical_binding_extension; eauto.
  - apply import_failed_encoded_attempt_preserves_the_entire_byte_store in Attempt. subst result.
    split.
    + intros selected query bytes Existing. exact Existing.
    + apply import_binding_extension_reflexive.
Qed.

Inductive import_encoded_run : ImportEncodedColdStore -> ImportEncodedColdStore -> Prop :=
| import_encoded_run_refl : forall store, import_encoded_run store store
| import_encoded_run_step : forall first middle last alias key input storage_ok success,
    import_encoded_run first middle ->
    import_encoded_attempt middle alias key input storage_ok = (last, success) ->
    import_encoded_run first last.

Theorem import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings :
  forall history first last,
  import_encoded_run first last ->
  import_encoded_extension first last /\
  import_binding_extension (import_encoded_logical_view history first)
    (import_encoded_logical_view history last).
Proof.
  intros history first last Run. induction Run as
    [store|first middle last alias key input storage_ok success Run IH Attempt].
  - split.
    + intros selected query bytes Existing. exact Existing.
    + apply import_binding_extension_reflexive.
  - destruct IH as [Bytes Bindings].
    destruct (import_encoded_attempt_preserves_physical_bytes_and_logical_bindings
      history _ _ _ _ _ _ _ Attempt) as [NextBytes NextBindings]. split.
    + intros selected query bytes Existing. apply NextBytes. now apply Bytes.
    + eapply import_binding_extension_transitive; eauto.
Qed.

Theorem import_encoded_run_preserves_strict_resolution : forall first last key leaf,
  import_encoded_run first last -> import_resolve_encoded_cold first key = Some (Some leaf) ->
  import_resolve_encoded_cold last key = Some (Some leaf).
Proof.
  intros first last key leaf Run Resolved.
  pose proof (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings
    (fun _ => None) _ _ Run) as [_ [_ Bindings]].
  apply (import_encoded_logical_resolution_matches_only_strict_success (fun _ => None)).
  apply Bindings.
  now apply import_encoded_logical_resolution_matches_only_strict_success.
Qed.

Theorem import_interleaved_encoded_alias_reads_preserve_the_decoded_leaf :
  forall initial first second key leaf,
  import_resolve_encoded_cold initial key = Some (Some leaf) ->
  import_encoded_run initial first -> import_encoded_run first second ->
  import_encoded_raw_first_read first second key = Some (Some leaf).
Proof.
  intros initial first second key leaf Resolved First Second.
  pose proof (import_encoded_run_preserves_strict_resolution _ _ _ _ First Resolved) as FirstResolved.
  apply import_encoded_strict_resolution_requires_actual_reader_and_both_guards in FirstResolved
    as [Read _].
  apply import_encoded_success_has_present_decodable_bytes in Read.
  unfold import_encoded_raw_first_read.
  destruct Read as [[bytes [Raw Parsed]]|[Raw [bytes [Legacy Parsed]]]].
  - rewrite Raw. cbn [import_decode_encoded_alias]. now rewrite Parsed.
  - rewrite Raw.
    destruct (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings
      (fun _ => None) _ _ Second) as [Bindings _].
    rewrite (Bindings ImportLegacyAlias key bytes Legacy).
    cbn [import_decode_encoded_alias]. now rewrite Parsed.
Qed.

Section TypedEncodedCold.

Variable Value : ImportLeafKind -> Type.
Variable decode_item : forall kind, list nat -> option (Value kind).

Theorem import_authenticated_encoded_commit_establishes_the_consuming_read :
  forall history history_hash Hash store alias key expected input result leaf,
  import_authenticate_cold_bytes Value decode_item Hash expected key input = Some leaf ->
  import_guarded_encoded_insert store alias key input = Some result ->
  import_encoded_extension store result /\
  import_binding_extension (import_encoded_logical_view history store)
    (import_encoded_logical_view history result) /\
  import_consumer_checked_read Value decode_item history_hash
    (fun payload => Hash (import_nat_to_key 8 (length payload) ++ payload))
    (import_encoded_logical_view history result) (ImportColdRef key expected) = Some [].
Proof.
  intros history history_hash Hash store alias key expected input result leaf Authenticated Inserted.
  pose proof (import_authenticated_cold_bytes_have_hash_kind_and_typed_values
    Value decode_item _ _ _ _ _ Authenticated) as [Parsed _].
  destruct (import_guarded_encoded_insert_preserves_bytes_and_resolves_the_inserted_leaf
    _ _ _ _ _ _ Parsed Inserted) as [Bytes Resolved].
  split; auto. split.
  - eapply import_guarded_encoded_insert_refines_logical_binding_extension; eauto.
  - eapply import_authenticated_stored_leaf_passes_the_consuming_scan; eauto.
    now apply import_encoded_logical_resolution_matches_only_strict_success.
Qed.

Theorem import_encoded_attempt_histories_preserve_all_readable_roots :
  forall history history_hash payload_hash first last roots,
  import_encoded_run first last ->
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash
    (import_encoded_logical_view history first)) roots ->
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash
    (import_encoded_logical_view history last)) roots.
Proof.
  intros history history_hash payload_hash first last roots Run Closed.
  pose proof (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings history _ _ Run)
    as [_ Bindings].
  induction Closed; constructor; auto.
  eapply import_binding_extension_preserves_consumable_closed_root; eauto.
Qed.

End TypedEncodedCold.

Definition import_encoded_bytes_eq_dec : forall first second : option (list nat),
  {first = second} + {first <> second}.
Proof. decide equality; apply list_eq_dec; apply Nat.eq_dec. Defined.

Definition import_encoded_observations_match (observed current : ImportEncodedColdStore)
    (key : nat) : bool :=
  if import_encoded_bytes_eq_dec (observed ImportRawAlias key) (current ImportRawAlias key) then
    if import_encoded_bytes_eq_dec (observed ImportLegacyAlias key) (current ImportLegacyAlias key)
      then true else false
  else false.

Theorem import_encoded_observations_match_exactly_both_aliases : forall observed current key,
  import_encoded_observations_match observed current key = true <->
  observed ImportRawAlias key = current ImportRawAlias key /\
  observed ImportLegacyAlias key = current ImportLegacyAlias key.
Proof.
  intros observed current key. unfold import_encoded_observations_match.
  destruct (import_encoded_bytes_eq_dec (observed ImportRawAlias key) (current ImportRawAlias key));
    [destruct (import_encoded_bytes_eq_dec
      (observed ImportLegacyAlias key) (current ImportLegacyAlias key))|];
    split; intros; try discriminate; intuition congruence.
Qed.

Theorem import_checked_observations_transfer_the_commit_guard :
  forall observed current alias key input candidate,
  import_guarded_encoded_insert observed alias key input = Some candidate ->
  import_encoded_observations_match observed current key = true ->
  import_guarded_encoded_insert current alias key input =
    Some (import_fill_encoded_alias current alias key input).
Proof.
  intros observed current alias key input candidate Accepted Matched.
  destruct (import_guarded_encoded_insert_has_checked_commit_inputs _ _ _ _ _ Accepted)
    as [leaf [Parsed [Raw [Legacy _]]]].
  apply import_encoded_observations_match_exactly_both_aliases in Matched as [SameRaw SameLegacy].
  unfold import_guarded_encoded_insert. rewrite Parsed, <- SameRaw, <- SameLegacy, Raw, Legacy.
  reflexivity.
Qed.

Definition import_observed_encoded_attempt (observed current : ImportEncodedColdStore)
    (alias : ImportColdAlias) (key : nat) (input : list nat) (storage_ok : bool)
    : ImportEncodedColdStore * bool :=
  match import_guarded_encoded_insert observed alias key input with
  | None => (current, false)
  | Some _ => if import_encoded_observations_match observed current key && storage_ok then
      (import_fill_encoded_alias current alias key input, true) else (current, false)
  end.

Theorem import_successful_observed_attempt_refines_the_guarded_commit :
  forall observed current alias key input storage_ok result,
  import_observed_encoded_attempt observed current alias key input storage_ok = (result, true) ->
  import_encoded_attempt current alias key input true = (result, true).
Proof.
  intros observed current alias key input storage_ok result Attempt.
  unfold import_observed_encoded_attempt in Attempt.
  destruct (import_guarded_encoded_insert observed alias key input) as [candidate|] eqn:Accepted;
    [|discriminate].
  destruct (_ && _) eqn:Ready; [|discriminate]. inversion Attempt; subst result.
  apply andb_true_iff in Ready as [Matched _]. unfold import_encoded_attempt.
  now rewrite (import_checked_observations_transfer_the_commit_guard _ _ _ _ _ _ Accepted Matched).
Qed.

Theorem import_failed_observed_attempt_preserves_the_commit_store :
  forall observed current alias key input storage_ok result,
  import_observed_encoded_attempt observed current alias key input storage_ok = (result, false) ->
  result = current.
Proof.
  intros observed current alias key input storage_ok result Attempt.
  unfold import_observed_encoded_attempt in Attempt.
  destruct (import_guarded_encoded_insert observed alias key input);
    [destruct (_ && _)|]; congruence.
Qed.

Theorem import_changed_alias_observation_prevents_commit :
  forall observed current alias key input storage_ok,
  (observed ImportRawAlias key <> current ImportRawAlias key \/
   observed ImportLegacyAlias key <> current ImportLegacyAlias key) ->
  import_observed_encoded_attempt observed current alias key input storage_ok = (current, false).
Proof.
  intros observed current alias key input storage_ok Changed.
  assert (import_encoded_observations_match observed current key = false) as Mismatch.
  { destruct (import_encoded_observations_match observed current key) eqn:Matched; auto.
    apply import_encoded_observations_match_exactly_both_aliases in Matched. tauto. }
  unfold import_observed_encoded_attempt.
  destruct (import_guarded_encoded_insert observed alias key input); auto.
  now rewrite Mismatch.
Qed.

Theorem import_observed_attempt_preserves_concurrent_unrelated_keys :
  forall observed current alias key input storage_ok result success selected query,
  query <> key ->
  import_observed_encoded_attempt observed current alias key input storage_ok = (result, success) ->
  result selected query = current selected query.
Proof.
  intros observed current alias key input storage_ok result success selected query Different Attempt.
  destruct success.
  - apply import_successful_observed_attempt_refines_the_guarded_commit in Attempt.
    unfold import_encoded_attempt in Attempt.
    destruct (import_guarded_encoded_insert current alias key input) as [candidate|] eqn:Accepted;
      [|discriminate]. inversion Attempt; subst candidate.
    destruct (import_guarded_encoded_insert_has_checked_commit_inputs _ _ _ _ _ Accepted)
      as [leaf [_ [_ [_ Same]]]]. subst result.
    now apply import_fill_alias_preserves_other_keys.
  - apply import_failed_observed_attempt_preserves_the_commit_store in Attempt. now subst result.
Qed.

Theorem import_every_observed_attempt_refines_a_guarded_run :
  forall observed current alias key input storage_ok result success,
  import_observed_encoded_attempt observed current alias key input storage_ok = (result, success) ->
  import_encoded_run current result.
Proof.
  intros observed current alias key input storage_ok result success Attempt. destruct success.
  - apply import_successful_observed_attempt_refines_the_guarded_commit in Attempt.
    eapply import_encoded_run_step; [constructor|exact Attempt].
  - apply import_failed_observed_attempt_preserves_the_commit_store in Attempt. subst result.
    constructor.
Qed.

Inductive import_observed_encoded_run : ImportEncodedColdStore -> ImportEncodedColdStore -> Prop :=
| import_observed_encoded_run_refl : forall store, import_observed_encoded_run store store
| import_observed_encoded_run_step : forall first current result observed alias key input storage_ok success,
    import_observed_encoded_run first current ->
    import_observed_encoded_attempt observed current alias key input storage_ok = (result, success) ->
    import_observed_encoded_run first result.

Theorem import_arbitrary_observed_attempts_refine_guarded_runs : forall first last,
  import_observed_encoded_run first last -> import_encoded_run first last.
Proof.
  intros first last Run. induction Run as
    [store|first current result observed alias key input storage_ok success Run IH Attempt].
  - constructor.
  - destruct success.
    + apply import_successful_observed_attempt_refines_the_guarded_commit in Attempt.
      eapply import_encoded_run_step; eauto.
    + apply import_failed_observed_attempt_preserves_the_commit_store in Attempt. now subst result.
Qed.

Theorem import_encoded_run_transitive : forall first middle last,
  import_encoded_run first middle -> import_encoded_run middle last -> import_encoded_run first last.
Proof.
  intros first middle last Before After. revert first Before.
  induction After as [store|middle current last alias key input storage_ok success After IH Attempt];
    intros first Before.
  - exact Before.
  - eapply import_encoded_run_step; [now apply IH|exact Attempt].
Qed.

Inductive ImportEncodedRow : Type :=
| import_encoded_row : ImportColdAlias -> nat -> list nat -> ImportEncodedRow.

Fixpoint import_stage_encoded_batch (store : ImportEncodedColdStore) (rows : list ImportEncodedRow)
    : option ImportEncodedColdStore :=
  match rows with
  | [] => Some store
  | import_encoded_row alias key input :: rest =>
      match import_guarded_encoded_insert store alias key input with
      | None => None
      | Some next => import_stage_encoded_batch next rest
      end
  end.

Theorem import_staged_encoded_batch_refines_guarded_attempts : forall rows store result,
  import_stage_encoded_batch store rows = Some result -> import_encoded_run store result.
Proof.
  induction rows as [|[alias key input] rest IH]; intros store result Staged.
  - cbn in Staged. inversion Staged; constructor.
  - cbn in Staged.
    destruct (import_guarded_encoded_insert store alias key input) as [next|] eqn:Accepted;
      [|discriminate].
    eapply import_encoded_run_transitive; [|eapply IH; exact Staged].
    eapply import_encoded_run_step with (middle := store) (storage_ok := true) (success := true).
    + constructor.
    + unfold import_encoded_attempt. now rewrite Accepted.
Qed.

Definition import_commit_encoded_batch (store : ImportEncodedColdStore)
    (rows : list ImportEncodedRow) (storage_ok : bool) : ImportEncodedColdStore * bool :=
  match import_stage_encoded_batch store rows with
  | None => (store, false)
  | Some candidate => if storage_ok then (candidate, true) else (store, false)
  end.

Theorem import_failed_encoded_batch_preserves_the_entire_commit_store :
  forall store rows storage_ok result,
  import_commit_encoded_batch store rows storage_ok = (result, false) -> result = store.
Proof.
  intros store rows storage_ok result Attempt. unfold import_commit_encoded_batch in Attempt.
  destruct (import_stage_encoded_batch store rows); [destruct storage_ok|]; congruence.
Qed.

Theorem import_every_encoded_batch_refines_a_guarded_run :
  forall store rows storage_ok result success,
  import_commit_encoded_batch store rows storage_ok = (result, success) ->
  import_encoded_run store result.
Proof.
  intros store rows storage_ok result success Attempt. destruct success.
  - unfold import_commit_encoded_batch in Attempt.
    destruct (import_stage_encoded_batch store rows) as [candidate|] eqn:Staged; [|discriminate].
    destruct storage_ok; [|discriminate]. inversion Attempt; subst candidate.
    eapply import_staged_encoded_batch_refines_guarded_attempts; eauto.
  - apply import_failed_encoded_batch_preserves_the_entire_commit_store in Attempt.
    subst result. constructor.
Qed.

Theorem import_equal_observations_survive_equal_staged_insertions :
  forall observed current alias key input query,
  import_encoded_observations_match observed current query = true ->
  import_encoded_observations_match
    (import_fill_encoded_alias observed alias key input)
    (import_fill_encoded_alias current alias key input) query = true.
Proof.
  intros observed current alias key input query Matched.
  apply import_encoded_observations_match_exactly_both_aliases in Matched as [Raw Legacy].
  apply import_encoded_observations_match_exactly_both_aliases. split.
  - unfold import_fill_encoded_alias.
    destruct (import_cold_alias_eq_dec ImportRawAlias alias);
      [destruct (Nat.eq_dec query key)|]; now rewrite Raw.
  - unfold import_fill_encoded_alias.
    destruct (import_cold_alias_eq_dec ImportLegacyAlias alias);
      [destruct (Nat.eq_dec query key)|]; now rewrite Legacy.
Qed.

Definition import_encoded_row_observation_matches (observed current : ImportEncodedColdStore)
    (row : ImportEncodedRow) : bool :=
  match row with import_encoded_row _ key _ => import_encoded_observations_match observed current key end.

Theorem import_checked_batch_observations_transfer_staging :
  forall rows observed current candidate,
  import_stage_encoded_batch observed rows = Some candidate ->
  Forall (fun row => import_encoded_row_observation_matches observed current row = true) rows ->
  exists result, import_stage_encoded_batch current rows = Some result.
Proof.
  induction rows as [|[alias key input] rest IH]; intros observed current candidate Staged Matched.
  - exists current. reflexivity.
  - cbn in Staged.
    destruct (import_guarded_encoded_insert observed alias key input) as [next|] eqn:Accepted;
      [|discriminate].
    inversion Matched as [|row tail Head Tail]; subst. cbn in Head.
    pose proof (import_checked_observations_transfer_the_commit_guard
      _ _ _ _ _ _ Accepted Head) as CurrentAccepted.
    destruct (import_guarded_encoded_insert_has_checked_commit_inputs _ _ _ _ _ Accepted)
      as [leaf [_ [_ [_ Same]]]]. subst next.
    assert (Forall (fun row => import_encoded_row_observation_matches
      (import_fill_encoded_alias observed alias key input)
      (import_fill_encoded_alias current alias key input) row = true) rest) as NextMatched.
    { eapply Forall_impl; [|exact Tail]. intros [selected query bytes] Same.
      cbn in *. now apply import_equal_observations_survive_equal_staged_insertions. }
    destruct (IH _ _ _ Staged NextMatched) as [result Result].
    exists result. cbn. now rewrite CurrentAccepted.
Qed.

Definition import_observed_encoded_batch (observed current : ImportEncodedColdStore)
    (rows : list ImportEncodedRow) (storage_ok : bool) : ImportEncodedColdStore * bool :=
  match import_stage_encoded_batch observed rows with
  | None => (current, false)
  | Some _ => if forallb (import_encoded_row_observation_matches observed current) rows && storage_ok
      then import_commit_encoded_batch current rows true else (current, false)
  end.

Theorem import_matched_valid_batch_cannot_fail_its_commit_state_guards :
  forall observed current rows candidate,
  import_stage_encoded_batch observed rows = Some candidate ->
  Forall (fun row => import_encoded_row_observation_matches observed current row = true) rows ->
  exists result, import_observed_encoded_batch observed current rows true = (result, true).
Proof.
  intros observed current rows candidate Staged Matched.
  destruct (import_checked_batch_observations_transfer_staging _ _ _ _ Staged Matched)
    as [result Result].
  assert (forallb (import_encoded_row_observation_matches observed current) rows = true) as All.
  { apply forallb_forall. now apply Forall_forall. }
  exists result. unfold import_observed_encoded_batch, import_commit_encoded_batch.
  now rewrite Staged, All, Result.
Qed.

Theorem import_observed_batches_preserve_definite_rollback_and_guarded_progress :
  forall observed current rows storage_ok result success,
  import_observed_encoded_batch observed current rows storage_ok = (result, success) ->
  import_encoded_run current result /\ (success = false -> result = current).
Proof.
  intros observed current rows storage_ok result success Attempt.
  unfold import_observed_encoded_batch in Attempt.
  destruct (import_stage_encoded_batch observed rows) as [candidate|];
    [destruct (_ && _)|]; try (inversion Attempt; subst; split; [constructor|auto]).
  split.
  - eapply import_every_encoded_batch_refines_a_guarded_run; eauto.
  - intros Failed. subst success.
    eapply import_failed_encoded_batch_preserves_the_entire_commit_store; eauto.
Qed.

Theorem import_encoded_view_preserves_joint_history_and_cold_extensions :
  forall old_history new_history first last,
  import_map_extension old_history new_history -> import_encoded_run first last ->
  import_binding_extension (import_encoded_logical_view old_history first)
    (import_encoded_logical_view new_history last).
Proof.
  intros old_history new_history first last History Run. split.
  - exact History.
  - intros key leaf Resolved.
    apply import_encoded_logical_resolution_matches_only_strict_success in Resolved.
    apply import_encoded_logical_resolution_matches_only_strict_success.
    eapply import_encoded_run_preserves_strict_resolution; eauto.
Qed.

Inductive import_encoded_storage_run :
    (nat -> option (list ImportRadixEdge)) -> ImportEncodedColdStore ->
    (nat -> option (list ImportRadixEdge)) -> ImportEncodedColdStore -> Prop :=
| import_encoded_storage_run_refl : forall history store,
    import_encoded_storage_run history store history store
| import_encoded_storage_cold_commit : forall initial_history initial history store rows storage_ok result success,
    import_encoded_storage_run initial_history initial history store ->
    import_commit_encoded_batch store rows storage_ok = (result, success) ->
    import_encoded_storage_run initial_history initial history result
| import_encoded_storage_history_commit : forall initial_history initial history store key edges result,
    import_encoded_storage_run initial_history initial history store ->
    import_guarded_history_insert (import_encoded_logical_view history store) key edges = Some result ->
    import_encoded_storage_run initial_history initial (import_history result) store
| import_encoded_storage_history_extension : forall initial_history initial history store next_history,
    import_encoded_storage_run initial_history initial history store ->
    import_map_extension history next_history ->
    import_encoded_storage_run initial_history initial next_history store.

Theorem import_observed_batch_extends_the_joint_storage_run :
  forall initial_history initial history observed current rows storage_ok result success,
  import_encoded_storage_run initial_history initial history current ->
  import_observed_encoded_batch observed current rows storage_ok = (result, success) ->
  import_encoded_storage_run initial_history initial history result.
Proof.
  intros initial_history initial history observed current rows storage_ok result success Run Attempt.
  unfold import_observed_encoded_batch in Attempt.
  destruct (import_stage_encoded_batch observed rows);
    [destruct (_ && _)|]; try (inversion Attempt; subst; exact Run).
  eapply import_encoded_storage_cold_commit; eauto.
Qed.

Theorem import_interleaved_history_and_cold_batches_preserve_both_binding_domains :
  forall first_history first last_history last,
  import_encoded_storage_run first_history first last_history last ->
  import_encoded_extension first last /\
  import_binding_extension (import_encoded_logical_view first_history first)
    (import_encoded_logical_view last_history last).
Proof.
  intros first_history first last_history last Run. induction Run as
    [history store|
     initial_history initial history store rows storage_ok result success Run IH Commit|
     initial_history initial history store key edges result Run IH Commit|
     initial_history initial history store next_history Run IH Extended].
  - split; [intros alias key bytes Found; exact Found|apply import_binding_extension_reflexive].
  - destruct IH as [Bytes Bindings].
    apply import_every_encoded_batch_refines_a_guarded_run in Commit.
    destruct (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings history _ _ Commit)
      as [NextBytes NextBindings]. split.
    + intros alias key bytes Found. apply NextBytes. now apply Bytes.
    + eapply import_binding_extension_transitive; eauto.
  - destruct IH as [Bytes Bindings]. split; auto.
    destruct (import_guarded_history_insert_preserves_exact_bindings_without_global_agreement
      _ _ _ _ Commit) as [[History _] _].
    eapply import_binding_extension_transitive; [exact Bindings|].
    apply import_encoded_view_preserves_joint_history_and_cold_extensions; [exact History|constructor].
  - destruct IH as [Bytes Bindings]. split; [exact Bytes|].
    eapply import_binding_extension_transitive; [exact Bindings|].
    apply import_encoded_view_preserves_joint_history_and_cold_extensions; [exact Extended|constructor].
Qed.

Theorem import_history_and_cold_commits_preserve_every_consumable_root :
  forall Value decode_item history_hash payload_hash first_history first last_history last roots,
  import_encoded_storage_run first_history first last_history last ->
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash
    (import_encoded_logical_view first_history first)) roots ->
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash
    (import_encoded_logical_view last_history last)) roots.
Proof.
  intros Value decode_item history_hash payload_hash first_history first last_history last roots Run Closed.
  destruct (import_interleaved_history_and_cold_batches_preserve_both_binding_domains _ _ _ _ Run)
    as [_ Bindings].
  induction Closed; constructor; auto.
  eapply import_binding_extension_preserves_consumable_closed_root; eauto.
Qed.

Theorem import_history_and_cold_commits_refine_a_consuming_scan_write :
  forall Value decode_item history_hash payload_hash first_history first last_history last checked frontier,
  import_encoded_storage_run first_history first last_history last ->
  import_consumer_scan_step Value decode_item history_hash payload_hash
    {| import_scan_store := import_encoded_logical_view first_history first;
       import_scan_checked := checked; import_scan_frontier := frontier |}
    {| import_scan_store := import_encoded_logical_view last_history last;
       import_scan_checked := checked; import_scan_frontier := frontier |}.
Proof.
  intros Value decode_item history_hash payload_hash first_history first last_history last checked frontier Run.
  apply import_consumer_scan_concurrent_commit.
  eapply import_interleaved_history_and_cold_batches_preserve_both_binding_domains in Run. tauto.
Qed.

Definition import_empty_encoded_store : ImportEncodedColdStore := fun _ _ => None.
Definition import_valid_empty_joins_bytes : list nat := repeat 0 4 ++ [8] ++ repeat 0 15.
Definition import_valid_empty_data_bytes : list nat := [1] ++ repeat 0 3 ++ [8] ++ repeat 0 15.

Example import_encoded_reader_does_not_fall_back_past_malformed_raw :
  let store := import_fill_encoded_alias
    (import_fill_encoded_alias import_empty_encoded_store ImportLegacyAlias 1 import_valid_empty_joins_bytes)
    ImportRawAlias 1 [99] in
  import_encoded_raw_first_read store store 1 = None /\ import_resolve_encoded_cold store 1 = None.
Proof. split; reflexivity. Qed.

Example import_strict_alias_resolution_is_stronger_than_the_legacy_reader :
  let store := import_fill_encoded_alias
    (import_fill_encoded_alias import_empty_encoded_store ImportLegacyAlias 1 [99])
    ImportRawAlias 1 import_valid_empty_joins_bytes in
  import_encoded_raw_first_read store store 1 =
    Some (Some {| import_leaf_kind := ImportJoins; import_leaf_payload := repeat 0 8 |}) /\
  import_resolve_encoded_cold store 1 = None.
Proof. split; reflexivity. Qed.

Example import_equivalent_outer_encoding_retains_the_original_stored_bytes :
  let original := import_valid_empty_joins_bytes ++ [91; 92] in
  let store := import_fill_encoded_alias import_empty_encoded_store ImportRawAlias 1 original in
  match import_guarded_encoded_insert store ImportRawAlias 1 import_valid_empty_joins_bytes with
  | Some result => result ImportRawAlias 1 = Some original
  | None => False
  end.
Proof. vm_compute. reflexivity. Qed.

Example import_stale_alias_preflight_permits_conflicting_target_only_commits :
  let first := import_fill_encoded_alias import_empty_encoded_store ImportRawAlias 1 import_valid_empty_joins_bytes in
  let unsafe := import_fill_encoded_alias first ImportLegacyAlias 1 import_valid_empty_data_bytes in
  import_guarded_encoded_insert import_empty_encoded_store ImportRawAlias 1 import_valid_empty_joins_bytes =
    Some first /\
  import_guarded_encoded_insert import_empty_encoded_store ImportLegacyAlias 1 import_valid_empty_data_bytes =
    Some (import_fill_encoded_alias import_empty_encoded_store ImportLegacyAlias 1 import_valid_empty_data_bytes) /\
  import_resolve_encoded_cold unsafe 1 = None /\
  import_guarded_encoded_insert first ImportLegacyAlias 1 import_valid_empty_data_bytes = None /\
  import_observed_encoded_attempt import_empty_encoded_store first ImportLegacyAlias 1
    import_valid_empty_data_bytes true = (first, false).
Proof. vm_compute. repeat split; reflexivity. Qed.

Example import_conflicting_repeated_batch_rows_never_publish_a_staged_prefix :
  import_commit_encoded_batch import_empty_encoded_store
    [import_encoded_row ImportRawAlias 1 import_valid_empty_joins_bytes;
     import_encoded_row ImportRawAlias 1 import_valid_empty_data_bytes] true =
    (import_empty_encoded_store, false).
Proof. vm_compute. reflexivity. Qed.

Example import_compatible_repeated_batch_rows_preserve_the_first_encoding :
  let original := import_valid_empty_joins_bytes ++ [91; 92] in
  let rows := [import_encoded_row ImportRawAlias 1 original;
               import_encoded_row ImportRawAlias 1 import_valid_empty_joins_bytes;
               import_encoded_row ImportLegacyAlias 1 import_valid_empty_joins_bytes] in
  let '(result, success) := import_commit_encoded_batch import_empty_encoded_store rows true in
  success = true /\ result ImportRawAlias 1 = Some original /\
  result ImportLegacyAlias 1 = Some import_valid_empty_joins_bytes.
Proof. vm_compute. repeat split; reflexivity. Qed.
