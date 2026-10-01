From Stdlib Require Import List Arith Bool Sorting.Permutation.
From CostAccountedRho Require Import StateImportClosure StateImportStorage StateImportCodec
  StateImportCursor StateImportTraversal StateImportPage StateImportExecution StateImportWire
  StateImportCold StateImportEncodedCold StateImportOccurrence StateImportOverlay.
Import ListNotations.

Definition ImportReceivedColdRow := (list nat * list nat)%type.

Definition import_compile_received_cold_row (row : ImportReceivedColdRow) : ImportEncodedRow :=
  import_encoded_row ImportRawAlias (import_key_to_nat (fst row)) (snd row).

Definition import_empty_received_cold : ImportEncodedColdStore := fun _ _ => None.

Definition import_received_cold_reader (store : ImportEncodedColdStore) (key : nat) : ImportRootLookup :=
  match store ImportRawAlias key with
  | None => ImportRootLookupAbsent
  | Some bytes => ImportRootLookupFound bytes
  end.

Definition import_received_decoded_value (store : ImportEncodedColdStore) (key : nat) : option ImportLeaf :=
  match store ImportRawAlias key with
  | None => None
  | Some bytes => import_read_persisted_leaf bytes
  end.

Definition import_occurrence_has_cold_key (key : nat) (entry : ImportLocatedEntry) : bool :=
  match import_located_reference entry with
  | Some (ImportColdRef found _) => key =? found
  | _ => false
  end.

Theorem import_occurrence_key_has_exact_reference : forall key entry,
  import_occurrence_has_cold_key key entry = true <->
  exists kind, import_located_reference entry = Some (ImportColdRef key kind).
Proof.
  intros key entry. unfold import_occurrence_has_cold_key.
  destruct (import_located_reference entry) as [[found prefix|found kind]|];
    try (split; [discriminate|intros [kind Impossible]; discriminate]).
  rewrite Nat.eqb_eq. split.
  - intros Same. subst found. eauto.
  - intros [expected Same]. now inversion Same.
Qed.

Theorem import_staged_batch_establishes_every_parsed_row : forall rows store result alias key bytes leaf,
  import_stage_encoded_batch store rows = Some result ->
  In (import_encoded_row alias key bytes) rows -> import_read_persisted_leaf bytes = Some leaf ->
  import_resolve_encoded_cold result key = Some (Some leaf).
Proof.
  induction rows as [|[selected query input] rest IH];
    intros store result alias key bytes leaf Staged Present Parsed; [contradiction|].
  cbn [import_stage_encoded_batch] in Staged.
  destruct (import_guarded_encoded_insert store selected query input) as [next|] eqn:Inserted;
    [|discriminate].
  destruct Present as [Same|Later].
  - inversion Same; subst selected query input.
    destruct (import_guarded_encoded_insert_preserves_bytes_and_resolves_the_inserted_leaf
      _ _ _ _ _ _ Parsed Inserted) as [_ Resolved].
    eapply import_encoded_run_preserves_strict_resolution; [|exact Resolved].
    eapply import_staged_encoded_batch_refines_guarded_attempts; eauto.
  - eapply IH; eauto.
Qed.

Theorem import_staged_raw_rows_retain_source_bytes : forall rows store result,
  import_stage_encoded_batch store (map import_compile_received_cold_row rows) = Some result ->
  forall alias key bytes, result alias key = Some bytes ->
  store alias key = Some bytes \/
  (alias = ImportRawAlias /\ exists raw_key, In (raw_key, bytes) rows /\ import_key_to_nat raw_key = key).
Proof.
  induction rows as [|[raw input] rest IH]; intros store result Staged alias key bytes Found.
  - cbn in Staged. inversion Staged; subst result. now left.
  - cbn [map import_compile_received_cold_row fst snd import_stage_encoded_batch] in Staged.
    destruct (import_guarded_encoded_insert store ImportRawAlias (import_key_to_nat raw) input)
      as [next|] eqn:Inserted; [|discriminate].
    destruct (IH _ _ Staged _ _ _ Found) as [Earlier|[Alias [raw_key [Present Key]]]].
    + destruct (import_guarded_encoded_insert_has_checked_commit_inputs _ _ _ _ _ Inserted)
        as [leaf [_ [_ [_ Next]]]]. subst next.
      unfold import_fill_encoded_alias in Earlier.
      destruct (import_cold_alias_eq_dec alias ImportRawAlias) as [Alias|Other]; [|now left].
      destruct (Nat.eq_dec key (import_key_to_nat raw)) as [Key|Other]; [|now left].
      destruct (store alias key) as [old|] eqn:Old.
      * inversion Earlier; subst old. now left.
      * inversion Earlier; subst input. right. split; [exact Alias|].
        exists raw. split; [now left|symmetry; exact Key].
    + right. split; [exact Alias|]. exists raw_key. split; [now right|exact Key].
Qed.

Theorem import_successful_observed_batch_has_current_staging :
  forall observed current rows storage_ok result,
  import_observed_encoded_batch observed current rows storage_ok = (result, true) ->
  import_stage_encoded_batch current rows = Some result.
Proof.
  intros observed current rows storage_ok result Attempt.
  unfold import_observed_encoded_batch in Attempt.
  destruct (import_stage_encoded_batch observed rows); [|discriminate].
  destruct (_ && _); [|discriminate]. unfold import_commit_encoded_batch in Attempt.
  destruct (import_stage_encoded_batch current rows) as [candidate|] eqn:Staged; [|discriminate].
  inversion Attempt; subst candidate. reflexivity.
Qed.

Definition import_encoded_contents_follow (reference store : ImportEncodedColdStore) : Prop :=
  forall alias key bytes, store alias key = Some bytes -> exists leaf,
    import_read_persisted_leaf bytes = Some leaf /\
    import_resolve_encoded_cold reference key = Some (Some leaf).

Definition import_encoded_row_follows (reference : ImportEncodedColdStore) (row : ImportEncodedRow) : Prop :=
  match row with import_encoded_row _ key bytes => exists leaf,
    import_read_persisted_leaf bytes = Some leaf /\
    import_resolve_encoded_cold reference key = Some (Some leaf)
  end.

Theorem import_reference_agreement_permits_guarded_insertion :
  forall reference store alias key bytes leaf,
  import_encoded_contents_follow reference store ->
  import_read_persisted_leaf bytes = Some leaf ->
  import_resolve_encoded_cold reference key = Some (Some leaf) ->
  import_guarded_encoded_insert store alias key bytes =
    Some (import_fill_encoded_alias store alias key bytes) /\
  import_encoded_contents_follow reference (import_fill_encoded_alias store alias key bytes).
Proof.
  intros reference store alias key bytes leaf Follows Parsed Resolved.
  assert (forall selected, import_encoded_alias_agrees (store selected key) leaf = true) as Guards.
  { intros selected. apply import_encoded_alias_guard_preserves_malformed_presence.
    destruct (store selected key) as [existing|] eqn:Existing; [|exact I].
    destruct (Follows _ _ _ Existing) as [found [Parse Resolve]].
    assert (found = leaf) by congruence. now subst found. }
  split.
  - unfold import_guarded_encoded_insert. now rewrite Parsed, !Guards.
  - intros selected query input Found. unfold import_fill_encoded_alias in Found.
    destruct (import_cold_alias_eq_dec selected alias);
      [destruct (Nat.eq_dec query key) as [Same|Other]|]; try (eapply Follows; exact Found).
    destruct (store selected query) as [old|] eqn:Old.
    + inversion Found; subst old. eapply Follows; exact Old.
    + inversion Found; subst input query. exists leaf. auto.
Qed.

Theorem import_reference_compatible_rows_admit_every_staging_order : forall rows reference store,
  Forall (import_encoded_row_follows reference) rows -> import_encoded_contents_follow reference store ->
  exists result, import_stage_encoded_batch store rows = Some result /\ import_encoded_contents_follow reference result.
Proof.
  induction rows as [|[alias key bytes] rest IH]; intros reference store All Follows.
  - exists store. split; [reflexivity|exact Follows].
  - inversion All as [|row tail Head Tail]; subst.
    destruct Head as [leaf [Parsed Resolved]].
    destruct (import_reference_agreement_permits_guarded_insertion reference store alias key bytes leaf Follows Parsed Resolved)
      as [Inserted Next].
    destruct (IH _ _ Tail Next) as [result [Staged Result]].
    exists result. split; [cbn; now rewrite Inserted|exact Result].
Qed.

Theorem import_received_staging_keeps_legacy_alias_absent : forall rows candidate key,
  import_stage_encoded_batch import_empty_received_cold (map import_compile_received_cold_row rows) = Some candidate ->
  candidate ImportLegacyAlias key = None.
Proof.
  intros rows candidate key Staged.
  destruct (candidate ImportLegacyAlias key) as [bytes|] eqn:Found; [|reflexivity].
  destruct (import_staged_raw_rows_retain_source_bytes _ _ _ Staged _ _ _ Found)
    as [Impossible|[Impossible _]]; discriminate.
Qed.

Theorem import_received_staging_resolution_has_raw_bytes : forall rows candidate key leaf,
  import_stage_encoded_batch import_empty_received_cold (map import_compile_received_cold_row rows) = Some candidate ->
  import_resolve_encoded_cold candidate key = Some (Some leaf) ->
  exists bytes, candidate ImportRawAlias key = Some bytes /\ import_read_persisted_leaf bytes = Some leaf.
Proof.
  intros rows candidate key leaf Staged Resolved.
  apply import_encoded_strict_resolution_requires_actual_reader_and_both_guards in Resolved as [Read _].
  apply import_encoded_success_has_present_decodable_bytes in Read.
  destruct Read as [Found|[_ [bytes [Legacy _]]]]; [exact Found|].
  rewrite (import_received_staging_keeps_legacy_alias_absent _ _ _ Staged) in Legacy. discriminate.
Qed.

Section ReceivedColdValidation.

Variable Value : ImportLeafKind -> Type.
Variable decode_item : forall kind, list nat -> option (Value kind).
Variable HashBytes : list nat -> list nat.

Definition import_received_hash (bytes : list nat) := import_key_to_nat (HashBytes bytes).

Definition import_received_leaf_matches_occurrence (key : nat) (leaf : ImportLeaf)
    (entry : ImportLocatedEntry) : bool :=
  match import_located_reference entry with
  | Some (ImportColdRef found kind) =>
      if key =? found then import_validate_cold_leaf Value decode_item kind leaf else true
  | _ => true
  end.

Definition import_check_received_cold_row (entries : list ImportLocatedEntry)
    (row : ImportReceivedColdRow) : option ImportLeaf :=
  let '(key, bytes) := row in
  if import_cursor_word_validb key && forallb (fun byte => byte <? 256) bytes then
    match import_read_persisted_leaf bytes with
    | None => None
    | Some leaf => if list_eq_dec Nat.eq_dec (HashBytes (import_leaf_hash_input leaf)) key then
        if existsb (import_occurrence_has_cold_key (import_key_to_nat key)) entries &&
           forallb (import_received_leaf_matches_occurrence (import_key_to_nat key) leaf) entries
        then Some leaf else None
      else None
    end
  else None.

Definition import_received_cold_row_validb (entries : list ImportLocatedEntry)
    (row : ImportReceivedColdRow) : bool :=
  match import_check_received_cold_row entries row with Some _ => true | None => false end.

Definition import_prepare_received_cold (entries : list ImportLocatedEntry)
    (rows : list ImportReceivedColdRow) : option ImportEncodedColdStore :=
  if forallb (import_received_cold_row_validb entries) rows then
    import_stage_encoded_batch import_empty_received_cold (map import_compile_received_cold_row rows)
  else None.

Definition import_check_received_cold_page (entries : list ImportLocatedEntry)
    (rows : list ImportReceivedColdRow) : option ImportEncodedColdStore :=
  match import_prepare_received_cold entries rows with
  | None => None
  | Some candidate =>
      match import_check_cold_occurrences Value decode_item import_received_hash
        (import_received_cold_reader candidate) entries with
      | ImportOccurrenceAccepted => Some candidate
      | _ => None
      end
  end.

Theorem import_received_row_has_exact_authorization : forall entries key bytes leaf,
  import_check_received_cold_row entries (key, bytes) = Some leaf ->
  length key = 32 /\ Forall import_wire_byte key /\ Forall import_wire_byte bytes /\
  import_read_persisted_leaf bytes = Some leaf /\ HashBytes (import_leaf_hash_input leaf) = key /\
  (exists entry kind, In entry entries /\
    import_located_reference entry = Some (ImportColdRef (import_key_to_nat key) kind)) /\
  (forall entry kind, In entry entries ->
    import_located_reference entry = Some (ImportColdRef (import_key_to_nat key) kind) ->
    import_authenticate_cold_bytes Value decode_item import_received_hash kind
      (import_key_to_nat key) bytes = Some leaf).
Proof.
  intros entries key bytes leaf Checked. unfold import_check_received_cold_row in Checked.
  destruct (import_cursor_word_validb key && forallb (fun byte => byte <? 256) bytes)
    eqn:Domain; [|discriminate].
  destruct (import_read_persisted_leaf bytes) as [parsed|] eqn:Parsed; [|discriminate].
  destruct (list_eq_dec Nat.eq_dec (HashBytes (import_leaf_hash_input parsed)) key) as [Hash|Wrong];
    [|discriminate].
  destruct (existsb (import_occurrence_has_cold_key (import_key_to_nat key)) entries &&
    forallb (import_received_leaf_matches_occurrence (import_key_to_nat key) parsed) entries)
    eqn:Authorized; [|discriminate]. inversion Checked; subst parsed.
  apply andb_true_iff in Domain as [Key Bytes].
  apply import_cursor_word_validity_characterization in Key as [Width Key].
  assert (Forall import_wire_byte bytes) as ByteDomain.
  { apply Forall_forall. intros byte Present. apply forallb_forall with (x := byte) in Bytes; auto.
    now apply Nat.ltb_lt in Bytes. }
  apply andb_true_iff in Authorized as [Exists All].
  repeat split; try assumption.
  - apply existsb_exists in Exists as [entry [Present Found]].
    apply import_occurrence_key_has_exact_reference in Found as [kind Reference]. eauto.
  - intros entry kind Present Reference.
    apply forallb_forall with (x := entry) in All; [|exact Present].
    unfold import_received_leaf_matches_occurrence in All. rewrite Reference, Nat.eqb_refl in All.
    unfold import_authenticate_cold_bytes, import_received_hash.
    now rewrite Parsed, Hash, Nat.eqb_refl, All.
Qed.

Theorem import_received_preparation_checks_every_original_row : forall entries rows candidate,
  import_prepare_received_cold entries rows = Some candidate ->
  Forall (fun row => exists leaf, import_check_received_cold_row entries row = Some leaf) rows /\
  import_stage_encoded_batch import_empty_received_cold (map import_compile_received_cold_row rows) = Some candidate.
Proof.
  intros entries rows candidate Prepared. unfold import_prepare_received_cold in Prepared.
  destruct (forallb _ rows) eqn:All; [|discriminate]. split; [|exact Prepared].
  apply Forall_forall. intros row Present. apply forallb_forall with (x := row) in All; auto.
  unfold import_received_cold_row_validb in All.
  destruct (import_check_received_cold_row entries row); [eauto|discriminate].
Qed.

Theorem import_received_preparation_retains_only_received_bytes : forall entries rows candidate key bytes,
  import_prepare_received_cold entries rows = Some candidate ->
  candidate ImportRawAlias key = Some bytes ->
  exists raw_key, In (raw_key, bytes) rows /\ import_key_to_nat raw_key = key.
Proof.
  intros entries rows candidate key bytes Prepared Found.
  apply import_received_preparation_checks_every_original_row in Prepared as [_ Staged].
  destruct (import_staged_raw_rows_retain_source_bytes _ _ _ Staged _ _ _ Found)
    as [Impossible|[_ Source]]; [discriminate|exact Source].
Qed.

Theorem import_received_preparation_has_no_durable_fallback : forall entries rows candidate key,
  import_prepare_received_cold entries rows = Some candidate ->
  (forall raw bytes, In (raw, bytes) rows -> import_key_to_nat raw <> key) ->
  import_received_cold_reader candidate key = ImportRootLookupAbsent.
Proof.
  intros entries rows candidate key Prepared Missing. unfold import_received_cold_reader.
  destruct (candidate ImportRawAlias key) as [bytes|] eqn:Found; [|reflexivity].
  destruct (import_received_preparation_retains_only_received_bytes _ _ _ _ _ Prepared Found)
    as [raw [Present Equal]]. exfalso. now apply (Missing raw bytes Present).
Qed.

Theorem import_accepted_received_page_preserves_checks : forall entries rows candidate,
  import_check_received_cold_page entries rows = Some candidate ->
  import_prepare_received_cold entries rows = Some candidate /\
  import_check_cold_occurrences Value decode_item import_received_hash
    (import_received_cold_reader candidate) entries = ImportOccurrenceAccepted.
Proof.
  intros entries rows candidate Checked. unfold import_check_received_cold_page in Checked.
  destruct (import_prepare_received_cold entries rows) as [prepared|] eqn:Prepared; [|discriminate].
  destruct (import_check_cold_occurrences _ _ _ _ _) eqn:Occurrences; try discriminate.
  inversion Checked; subst prepared. auto.
Qed.

Theorem import_accepted_received_occurrence_has_an_original_row :
  forall entries rows candidate entry key kind,
  import_check_received_cold_page entries rows = Some candidate -> In entry entries ->
  import_located_reference entry = Some (ImportColdRef key kind) ->
  exists raw bytes leaf values,
    In (raw, bytes) rows /\ import_key_to_nat raw = key /\
    length raw = 32 /\ Forall import_wire_byte raw /\ Forall import_wire_byte bytes /\
    import_read_persisted_leaf bytes = Some leaf /\ HashBytes (import_leaf_hash_input leaf) = raw /\
    import_leaf_kind leaf = kind /\ import_consume_cold_leaf Value decode_item kind leaf = Some values.
Proof.
  intros entries rows candidate entry key kind Checked Present Reference.
  apply import_accepted_received_page_preserves_checks in Checked as [Prepared Occurrences].
  destruct (import_accepted_occurrence_has_consumable_payload Value decode_item import_received_hash
    _ _ _ _ _ Occurrences Present Reference) as [bytes [leaf [values [Read [Parsed [_ [Kind Consumed]]]]]]].
  unfold import_received_cold_reader in Read.
  destruct (candidate ImportRawAlias key) as [stored|] eqn:Found; [|discriminate].
  inversion Read; subst stored.
  destruct (import_received_preparation_retains_only_received_bytes _ _ _ _ _ Prepared Found)
    as [raw [Row Key]].
  apply import_received_preparation_checks_every_original_row in Prepared as [All _].
  rewrite Forall_forall in All. destruct (All (raw, bytes) Row) as [parsed Checked].
  apply import_received_row_has_exact_authorization in Checked as [Width [Domain [Bytes [Parse [Hash Rest]]]]].
  assert (parsed = leaf) by congruence. subst parsed. exists raw, bytes, leaf, values.
  repeat split; assumption.
Qed.

Theorem import_received_rows_cannot_hide_invalid_duplicates : forall entries rows candidate row,
  import_check_received_cold_page entries rows = Some candidate -> In row rows ->
  exists leaf, import_check_received_cold_row entries row = Some leaf.
Proof.
  intros entries rows candidate row Checked Present.
  apply import_accepted_received_page_preserves_checks in Checked as [Prepared _].
  apply import_received_preparation_checks_every_original_row in Prepared as [All _].
  rewrite Forall_forall in All. now apply All.
Qed.

Theorem import_accepted_received_page_has_exact_cold_key_coverage : forall entries rows candidate key,
  import_check_received_cold_page entries rows = Some candidate ->
  ((exists raw bytes, In (raw, bytes) rows /\ import_key_to_nat raw = key) <->
   exists entry kind, In entry entries /\ import_located_reference entry = Some (ImportColdRef key kind)).
Proof.
  intros entries rows candidate key Checked. split.
  - intros [raw [bytes [Present Key]]].
    destruct (import_received_rows_cannot_hide_invalid_duplicates _ _ _ _ Checked Present)
      as [leaf Row].
    apply import_received_row_has_exact_authorization in Row as [_ [_ [_ [_ [_ [Occurrence _]]]]]].
    now rewrite Key in Occurrence.
  - intros [entry [kind [Present Reference]]].
    destruct (import_accepted_received_occurrence_has_an_original_row _ _ _ _ _ _ Checked Present Reference)
      as [raw [bytes [leaf [values [Row [Key Rest]]]]]]. eauto.
Qed.

Theorem import_accepted_received_keys_have_no_numeric_aliases :
  forall entries rows candidate first first_bytes second second_bytes,
  import_check_received_cold_page entries rows = Some candidate ->
  In (first, first_bytes) rows -> In (second, second_bytes) rows ->
  import_key_to_nat first = import_key_to_nat second -> first = second.
Proof.
  intros entries rows candidate first first_bytes second second_bytes Checked First Second Equal.
  destruct (import_received_rows_cannot_hide_invalid_duplicates _ _ _ _ Checked First) as [left Left].
  destruct (import_received_rows_cannot_hide_invalid_duplicates _ _ _ _ Checked Second) as [right Right].
  apply import_received_row_has_exact_authorization in Left as [LeftWidth [LeftBytes _]].
  apply import_received_row_has_exact_authorization in Right as [RightWidth [RightBytes _]].
  eapply import_equal_width_key_conversion_has_no_aliases; eauto. now rewrite LeftWidth, RightWidth.
Qed.

Theorem import_received_preparation_permits_every_row_permutation : forall entries rows first reordered,
  import_prepare_received_cold entries rows = Some first -> Permutation rows reordered ->
  exists last, import_prepare_received_cold entries reordered = Some last.
Proof.
  intros entries rows first reordered Prepared Reordered.
  apply import_received_preparation_checks_every_original_row in Prepared as [All Staged].
  assert (Forall (fun row => exists leaf, import_check_received_cold_row entries row = Some leaf) reordered)
    as ReorderedChecks by (eapply Permutation_Forall; eauto).
  assert (Forall (import_encoded_row_follows first) (map import_compile_received_cold_row reordered)) as Follows.
  { apply Forall_forall. intros compiled Present. apply in_map_iff in Present as [[raw bytes] [Same Present]].
    subst compiled. cbn [import_encoded_row_follows import_compile_received_cold_row fst snd].
    rewrite Forall_forall in ReorderedChecks. destruct (ReorderedChecks _ Present) as [leaf Checked].
    apply import_received_row_has_exact_authorization in Checked as [_ [_ [_ [Parsed _]]]].
    exists leaf. split; [exact Parsed|].
    eapply import_staged_batch_establishes_every_parsed_row; [exact Staged| |exact Parsed].
    apply in_map_iff. exists (raw, bytes). split; [reflexivity|].
    eapply Permutation_in; [apply Permutation_sym; exact Reordered|exact Present]. }
  assert (import_encoded_contents_follow first import_empty_received_cold) as Empty.
  { intros alias key bytes Impossible. discriminate. }
  destruct (import_reference_compatible_rows_admit_every_staging_order _ _ _ Follows Empty)
    as [last [Last Follow]].
  exists last. unfold import_prepare_received_cold.
  assert (forallb (import_received_cold_row_validb entries) reordered = true) as Valid.
  { apply forallb_forall. intros row Present. rewrite Forall_forall in ReorderedChecks.
    destruct (ReorderedChecks _ Present) as [leaf Checked].
    unfold import_received_cold_row_validb. now rewrite Checked. }
  now rewrite Valid.
Qed.

Theorem import_received_occurrence_checks_survive_row_permutation :
  forall entries rows first reordered last,
  import_check_received_cold_page entries rows = Some first -> Permutation rows reordered ->
  import_prepare_received_cold entries reordered = Some last ->
  import_check_cold_occurrences Value decode_item import_received_hash
    (import_received_cold_reader last) entries = ImportOccurrenceAccepted.
Proof.
  intros entries rows first reordered last Accepted Reordered Prepared.
  pose proof (import_accepted_received_page_preserves_checks _ _ _ Accepted) as [_ Checked].
  apply import_occurrence_checks_cover_every_occurrence in Checked.
  apply import_occurrence_checks_cover_every_occurrence. apply Forall_forall.
  intros entry Present. rewrite Forall_forall in Checked. specialize (Checked entry Present).
  destruct (import_located_reference entry) as [[key context|key kind]|] eqn:Reference.
  - unfold import_check_cold_occurrence. now rewrite Reference.
  - destruct (import_accepted_received_occurrence_has_an_original_row _ _ _ _ _ _ Accepted Present Reference)
      as [raw [bytes [leaf [values [Row [Key [_ [_ [_ [Parsed [Hash [Kind Consumed]]]]]]]]]]]].
    apply import_received_preparation_checks_every_original_row in Prepared as [_ Staged].
    assert (import_resolve_encoded_cold last key = Some (Some leaf)) as Resolved.
    { eapply import_staged_batch_establishes_every_parsed_row; [exact Staged| |exact Parsed].
      rewrite <- Key. apply in_map_iff. exists (raw, bytes). split; [reflexivity|].
      eapply Permutation_in; eauto. }
    destruct (import_received_staging_resolution_has_raw_bytes _ _ _ _ Staged Resolved)
      as [stored [Found Stored]].
    apply (import_cold_occurrence_success_is_exact _ _ _ _ _ _ _ Reference).
    exists stored, leaf. split; [unfold import_received_cold_reader; now rewrite Found|].
    unfold import_authenticate_cold_bytes, import_received_hash. rewrite Stored, Hash, Key, Nat.eqb_refl.
    unfold import_validate_cold_leaf. now rewrite Consumed.
  - unfold import_check_cold_occurrence in Checked. rewrite Reference in Checked. discriminate.
Qed.

Theorem import_received_page_acceptance_is_permutation_invariant : forall entries rows first reordered,
  import_check_received_cold_page entries rows = Some first -> Permutation rows reordered ->
  exists last, import_check_received_cold_page entries reordered = Some last.
Proof.
  intros entries rows first reordered Accepted Reordered.
  pose proof (import_accepted_received_page_preserves_checks _ _ _ Accepted) as [Prepared _].
  destruct (import_received_preparation_permits_every_row_permutation _ _ _ _ Prepared Reordered)
    as [last Last].
  pose proof (import_received_occurrence_checks_survive_row_permutation _ _ _ _ _ Accepted Reordered Last) as Checked.
  exists last. unfold import_check_received_cold_page. now rewrite Last, Checked.
Qed.

Theorem import_received_decoded_bindings_survive_permutation : forall entries rows first reordered last key leaf,
  import_prepare_received_cold entries rows = Some first -> Permutation rows reordered ->
  import_prepare_received_cold entries reordered = Some last ->
  import_received_decoded_value first key = Some leaf -> import_received_decoded_value last key = Some leaf.
Proof.
  intros entries rows first reordered last key leaf First Reordered Last Decoded.
  unfold import_received_decoded_value in Decoded.
  destruct (first ImportRawAlias key) as [bytes|] eqn:Found; [|discriminate].
  destruct (import_received_preparation_retains_only_received_bytes _ _ _ _ _ First Found)
    as [raw [Present Key]].
  apply import_received_preparation_checks_every_original_row in Last as [_ Staged].
  assert (import_resolve_encoded_cold last key = Some (Some leaf)) as Resolved.
  { eapply import_staged_batch_establishes_every_parsed_row; [exact Staged| |exact Decoded].
    rewrite <- Key. apply in_map_iff. exists (raw, bytes). split; [reflexivity|].
    eapply Permutation_in; eauto. }
  destruct (import_received_staging_resolution_has_raw_bytes _ _ _ _ Staged Resolved)
    as [stored [Read Parsed]].
  unfold import_received_decoded_value. now rewrite Read.
Qed.

Theorem import_received_permutations_preserve_decoded_maps_not_trailer_bytes :
  forall entries rows first reordered last,
  import_prepare_received_cold entries rows = Some first -> Permutation rows reordered ->
  import_prepare_received_cold entries reordered = Some last ->
  forall key, import_received_decoded_value first key = import_received_decoded_value last key.
Proof.
  intros entries rows first reordered last First Reordered Last key.
  destruct (import_received_decoded_value first key) as [leaf|] eqn:Left.
  - symmetry. exact (import_received_decoded_bindings_survive_permutation _ _ _ _ _ _ _
      First Reordered Last Left).
  - destruct (import_received_decoded_value last key) as [leaf|] eqn:Right; [|reflexivity].
    pose proof (import_received_decoded_bindings_survive_permutation
      _ _ _ _ _ _ _ Last (Permutation_sym Reordered) First Right) as Same. congruence.
Qed.

Theorem import_received_batch_establishes_every_authorized_occurrence :
  forall entries rows candidate observed current storage_ok result history history_hash entry key kind,
  import_check_received_cold_page entries rows = Some candidate ->
  import_observed_encoded_batch observed current (map import_compile_received_cold_row rows) storage_ok
    = (result, true) ->
  In entry entries -> import_located_reference entry = Some (ImportColdRef key kind) ->
  import_consumer_checked_read Value decode_item history_hash
    (fun payload => import_received_hash (import_nat_to_key 8 (length payload) ++ payload))
    (import_encoded_logical_view history result) (ImportColdRef key kind) = Some [].
Proof.
  intros entries rows candidate observed current storage_ok result history history_hash entry key kind
    Checked Committed Present Reference.
  destruct (import_accepted_received_occurrence_has_an_original_row
    _ _ _ _ _ _ Checked Present Reference)
    as [raw [bytes [leaf [values [Row [Key [_ [_ [_ [Parsed [Hash [Kind Consumed]]]]]]]]]]]].
  apply import_successful_observed_batch_has_current_staging in Committed.
  assert (import_resolve_encoded_cold result key = Some (Some leaf)) as Resolved.
  { eapply import_staged_batch_establishes_every_parsed_row; [exact Committed| |exact Parsed].
    rewrite <- Key. apply in_map_iff. exists (raw, bytes). split; [reflexivity|exact Row]. }
  eapply import_authenticated_stored_leaf_passes_the_consuming_scan.
  - unfold import_authenticate_cold_bytes, import_received_hash. rewrite Parsed, Hash, Key, Nat.eqb_refl.
    unfold import_validate_cold_leaf. now rewrite Consumed.
  - now apply import_encoded_logical_resolution_matches_only_strict_success.
Qed.

Theorem import_canonical_cursor_context_reaches_guarded_cold_consumption :
  forall Hash before history_rows candidate last input skip budget entries final computed request start next
    cold_rows original origin,
  import_prepare_history_overlay Hash before history_rows = ImportHistoryOverlayReady candidate ->
  import_interleaved_cursor_export (import_overlay_traversal_reader Hash candidate)
    (import_overlay_traversal_reader Hash last) input skip budget entries final ->
  import_cursor_encode input = Some request ->
  import_adapt_wire_result input (ImportEvaluationSuccess entries final) = ImportWireSuccess computed ->
  import_check_reply_metadata request start next (map (fun row => import_key_to_nat (fst row)) history_rows)
    (map (fun row => import_key_to_nat (fst row)) cold_rows) computed = true ->
  import_history_path (import_overlay_traversal_reader Hash candidate) original origin (import_cursor_root input) ->
  exists located,
    import_located_cursor_export (import_overlay_traversal_reader Hash last) origin input skip budget located final /\
    map import_located_entry located = entries /\
    Forall (import_complete_occurrence_witness (import_overlay_traversal_reader Hash last) original) located /\
    import_cursor_encode (import_wire_next_cursor input (map import_located_entry located) final) = Some next /\
    (forall cold_candidate observed current storage_ok result history history_hash,
      import_check_received_cold_page located cold_rows = Some cold_candidate ->
      import_observed_encoded_batch observed current (map import_compile_received_cold_row cold_rows) storage_ok
        = (result, true) ->
      forall entry key kind, In entry located -> import_located_reference entry = Some (ImportColdRef key kind) ->
      import_complete_occurrence_witness (import_overlay_traversal_reader Hash last) original entry /\
      import_consumer_checked_read Value decode_item history_hash
        (fun payload => import_received_hash (import_nat_to_key 8 (length payload) ++ payload))
        (import_encoded_logical_view history result) (ImportColdRef key kind) = Some []).
Proof.
  intros Hash before history_rows candidate last input skip budget entries final computed request start next
    cold_rows original origin Prepared Run Request Adapted Metadata Origin.
  destruct (import_checked_cursor_reply_preserves_exact_occurrences_and_singleton_origin
    _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ Prepared Run Request Adapted Metadata Origin)
    as [located [Located [Erase [Complete [Next Singleton]]]]].
  exists located. split; [exact Located|]. split; [exact Erase|].
  split; [exact Complete|]. split; [exact Next|].
  intros cold_candidate observed current storage_ok result history history_hash Checked Committed entry key kind
    Present Reference. split.
  - rewrite Forall_forall in Complete. now apply Complete.
  - eapply import_received_batch_establishes_every_authorized_occurrence; eauto.
Qed.

End ReceivedColdValidation.

Definition import_received_empty_join_occurrence : ImportLocatedEntry :=
  {| import_located_entry := ImportExportLeaf 0; import_located_path := [2; 9] |}.

Example import_received_compatible_trailers_keep_first_bytes_in_each_order :
  let key := repeat 0 32 in
  let first := import_valid_empty_joins_bytes in
  let second := first ++ [99] in
  let prepare := import_prepare_received_cold (fun _ => unit) (fun _ _ => Some tt)
    (fun _ => key) [import_received_empty_join_occurrence] in
  match prepare [(key, first); (key, second)], prepare [(key, second); (key, first)] with
  | Some first_map, Some last_map =>
      first_map ImportRawAlias 0 = Some first /\ last_map ImportRawAlias 0 = Some second /\
      import_received_decoded_value first_map 0 = import_received_decoded_value last_map 0
  | _, _ => False
  end.
Proof. vm_compute. auto. Qed.

Example import_received_conflicting_leaves_reject_even_with_a_constant_hash :
  let key := repeat 0 32 in
  let first := (key, import_valid_empty_joins_bytes) in
  let second := (key, repeat 0 4 ++ [9] ++ repeat 0 15 ++ [99]) in
  let entries := [import_received_empty_join_occurrence] in
  let valid := import_received_cold_row_validb (fun _ => unit) (fun _ _ => Some tt) (fun _ => key) entries in
  let prepare := import_prepare_received_cold (fun _ => unit) (fun _ _ => Some tt) (fun _ => key) entries in
  valid first = true /\ valid second = true /\ prepare [first; second] = None /\ prepare [second; first] = None.
Proof. vm_compute. auto. Qed.
