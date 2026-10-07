From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportClosure StateImportStorage StateImportWire
  StateImportCodec StateImportCold StateImportEncodedCold StateImportObservations.
Import ListNotations.

Definition ImportColdLocation := (ImportColdAlias * nat)%type.

Definition import_retry_locations (keys : list nat) : list ImportColdLocation :=
  flat_map (fun key => [(ImportRawAlias, key); (ImportLegacyAlias, key)]) keys.

Definition import_physical_alias_key (alias : ImportColdAlias) (bytes : list nat) : list nat :=
  match alias with
  | ImportRawAlias => bytes
  | ImportLegacyAlias => import_nat_to_key 8 (length bytes) ++ bytes
  end.

Theorem import_valid_physical_alias_encodings_are_disjoint_and_injective :
  forall first_alias second_alias first second,
  length first = 32 -> length second = 32 ->
  import_physical_alias_key first_alias first = import_physical_alias_key second_alias second ->
  first_alias = second_alias /\ first = second.
Proof.
  intros first_alias second_alias first second FirstWidth SecondWidth Same.
  destruct first_alias, second_alias.
  - auto.
  - change (first = import_nat_to_key 8 (length second) ++ second) in Same.
    apply (f_equal (@length nat)) in Same.
    rewrite length_app, import_key_conversion_preserves_exact_width, FirstWidth, SecondWidth in Same. lia.
  - change (import_nat_to_key 8 (length first) ++ first = second) in Same.
    apply (f_equal (@length nat)) in Same.
    rewrite length_app, import_key_conversion_preserves_exact_width, FirstWidth, SecondWidth in Same. lia.
  - change (import_nat_to_key 8 (length first) ++ first =
      import_nat_to_key 8 (length second) ++ second) in Same.
    rewrite FirstWidth, SecondWidth in Same. apply app_inv_head in Same. auto.
Qed.

Fixpoint import_absent_locations (locations : list ImportColdLocation)
    (store : ImportEncodedColdStore) : nat :=
  match locations with
  | [] => 0
  | (alias, key) :: rest =>
      match store alias key with
      | None => S (import_absent_locations rest store)
      | Some _ => import_absent_locations rest store
      end
  end.

Definition import_location_extension (locations : list ImportColdLocation)
    (first last : ImportEncodedColdStore) : Prop :=
  forall alias key bytes, In (alias, key) locations ->
    first alias key = Some bytes -> last alias key = Some bytes.

Definition import_location_mismatch (locations : list ImportColdLocation)
    (observed current : ImportEncodedColdStore) : Prop :=
  exists alias key, In (alias, key) locations /\ observed alias key <> current alias key.

Theorem import_absence_budget_is_bounded_by_physical_locations : forall locations store,
  import_absent_locations locations store <= length locations.
Proof.
  induction locations as [|[alias key] rest IH]; intros store; simpl; [lia|].
  specialize (IH store). destruct (store alias key); lia.
Qed.

Theorem import_two_alias_formats_give_two_locations_per_key : forall keys,
  length (import_retry_locations keys) = 2 * length keys.
Proof. induction keys; simpl; lia. Qed.

Theorem import_byte_preserving_extensions_cannot_increase_absence : forall locations first last,
  import_location_extension locations first last ->
  import_absent_locations locations last <= import_absent_locations locations first.
Proof.
  induction locations as [|[alias key] rest IH]; intros first last Extended; simpl; [lia|].
  assert (import_location_extension rest first last) as Tail.
  { intros selected query bytes Member Found. apply Extended; simpl; auto. }
  specialize (IH first last Tail).
  destruct (first alias key) as [bytes|] eqn:Found.
  - rewrite (Extended alias key bytes (or_introl eq_refl) Found). exact IH.
  - destruct (last alias key); lia.
Qed.

Theorem import_changed_guard_under_extension_consumes_absence : forall locations first last,
  import_location_extension locations first last ->
  import_location_mismatch locations first last ->
  import_absent_locations locations last < import_absent_locations locations first.
Proof.
  induction locations as [|[alias key] rest IH]; intros first last Extended [selected [query [Member Different]]].
  - contradiction.
  - assert (import_location_extension rest first last) as Tail.
    { intros other item bytes Included Found. apply Extended; simpl; auto. }
    pose proof (import_byte_preserving_extensions_cannot_increase_absence rest first last Tail) as Bound.
    simpl in Member. destruct Member as [Same|Member].
    + inversion Same; subst selected query. simpl.
      destruct (first alias key) as [bytes|] eqn:Found.
      * exfalso. apply Different. symmetry. apply Extended; simpl; auto.
      * destruct (last alias key); [lia|contradiction].
    + assert (import_location_mismatch rest first last) as Mismatch by (exists selected, query; auto).
      specialize (IH first last Tail Mismatch). simpl.
      destruct (first alias key) as [bytes|] eqn:Found.
      * rewrite (Extended alias key bytes (or_introl eq_refl) Found). exact IH.
      * destruct (last alias key); lia.
Qed.

Theorem import_physical_trace_retains_every_observed_byte : forall initial records current observations,
  import_cold_observation_trace initial records current observations ->
  import_encoded_extension (import_project_cold_observations observations) current.
Proof.
  intros initial records current observations Trace. induction Trace as
    [|records store observations alias key Trace IH Unread
     |records store observations alias key Trace IH Unread
     |records store observations next Trace IH Run].
  - intros selected query bytes Found. discriminate.
  - intros selected query bytes Found.
    unfold import_project_cold_observations, import_capture_first_cold_observation in Found.
    destruct (import_cold_alias_eq_dec selected alias) as [SameAlias|OtherAlias];
      [destruct (Nat.eq_dec query key) as [SameKey|OtherKey]|].
    + subst selected query. rewrite Unread in Found.
      unfold import_successful_cold_lookup in Found.
      destruct (store alias key) as [stored|] eqn:Stored; simpl in Found; congruence.
    + apply IH. exact Found.
    + apply IH. exact Found.
  - intros selected query bytes Found.
    unfold import_project_cold_observations, import_capture_first_cold_observation in Found.
    destruct (import_cold_alias_eq_dec selected alias) as [SameAlias|OtherAlias];
      [destruct (Nat.eq_dec query key) as [SameKey|OtherKey]|].
    + subst selected query. rewrite Unread in Found. discriminate.
    + apply IH. exact Found.
    + apply IH. exact Found.
  - destruct (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings
      (fun _ => None) _ _ Run) as [Extended _].
    intros alias key bytes Found. apply Extended. now apply IH.
Qed.

Theorem import_successful_physical_refresh_observes_initial_bytes : forall initial records current observations,
  import_cold_observation_trace initial records current observations ->
  forall alias key bytes, initial alias key = Some bytes ->
    import_cold_observation_ready (observations alias key) = true ->
    import_project_cold_observations observations alias key = Some bytes.
Proof.
  intros initial records current observations Trace. induction Trace as
    [|records store observations alias key Trace IH Unread
     |records store observations alias key Trace IH Unread
     |records store observations next Trace IH Run]; intros selected query bytes Initial Ready.
  - discriminate.
  - unfold import_project_cold_observations, import_capture_first_cold_observation in *.
    destruct (import_cold_alias_eq_dec selected alias) as [SameAlias|OtherAlias];
      [destruct (Nat.eq_dec query key) as [SameKey|OtherKey]|].
    + subst selected query. rewrite Unread.
      pose proof (import_observation_trace_preserves_the_concurrent_storage_run _ _ _ _ Trace) as Run.
      destruct (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings
        (fun _ => None) _ _ Run) as [Extended _].
      unfold import_successful_cold_lookup. rewrite (Extended alias key bytes Initial). reflexivity.
    + now apply IH.
    + now apply IH.
  - unfold import_project_cold_observations, import_capture_first_cold_observation in *.
    destruct (import_cold_alias_eq_dec selected alias) as [SameAlias|OtherAlias];
      [destruct (Nat.eq_dec query key) as [SameKey|OtherKey]|].
    + subst selected query. rewrite Unread in Ready. discriminate.
    + now apply IH.
    + now apply IH.
  - now apply IH.
Qed.

Definition import_locations_ready (locations : list ImportColdLocation)
    (observations : ImportColdObservations) : Prop :=
  forall alias key, In (alias, key) locations ->
    import_cold_observation_ready (observations alias key) = true.

Theorem import_fresh_reads_after_definite_conflict_strictly_reduce_retry_budget :
  forall locations observed current conflicted records refreshed observations,
  import_location_extension locations observed current ->
  import_encoded_run current conflicted ->
  import_location_mismatch locations observed conflicted ->
  import_cold_observation_trace conflicted records refreshed observations ->
  import_locations_ready locations observations ->
  import_absent_locations locations (import_project_cold_observations observations) <
    import_absent_locations locations observed /\
  import_location_extension locations (import_project_cold_observations observations) refreshed.
Proof.
  intros locations observed current conflicted records refreshed observations
    Previous Run [alias [key [Member Different]]] Trace Ready.
  destruct (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings
    (fun _ => None) _ _ Run) as [Extended _].
  assert (import_location_extension locations observed conflicted) as BeforeConflict.
  { intros selected query bytes Included Found. apply Extended. eapply Previous; eauto. }
  assert (import_location_extension locations conflicted (import_project_cold_observations observations)) as AfterConflict.
  { intros selected query bytes Included Found.
    eapply import_successful_physical_refresh_observes_initial_bytes; eauto. }
  split.
  - eapply import_changed_guard_under_extension_consumes_absence.
    + intros selected query bytes Included Found. eapply AfterConflict; [exact Included|].
      eapply BeforeConflict; eauto.
    + exists alias, key. split; [exact Member|].
      destruct (observed alias key) as [bytes|] eqn:Old.
      * exfalso. apply Different. symmetry. eapply BeforeConflict; eauto.
      * destruct (conflicted alias key) as [bytes|] eqn:Changed; [|contradiction].
        rewrite (AfterConflict alias key bytes Member Changed). discriminate.
  - intros selected query bytes Included Found.
    eapply import_physical_trace_retains_every_observed_byte; eauto.
Qed.

Inductive import_physical_retry_run (locations : list ImportColdLocation)
    (initial : ImportEncodedColdStore) :
    ImportEncodedColdStore -> ImportEncodedColdStore -> nat -> Prop :=
| import_physical_retry_begin : forall current,
    import_location_extension locations initial current ->
    import_physical_retry_run locations initial initial current 0
| import_physical_retry_conflict : forall observed current conflicts conflicted records refreshed observations,
    import_physical_retry_run locations initial observed current conflicts ->
    import_encoded_run current conflicted ->
    import_location_mismatch locations observed conflicted ->
    import_cold_observation_trace conflicted records refreshed observations ->
    import_locations_ready locations observations ->
    import_physical_retry_run locations initial
      (import_project_cold_observations observations) refreshed (S conflicts).

Theorem import_initial_physical_trace_establishes_the_retry_premise :
  forall locations initial records current observations,
  import_cold_observation_trace initial records current observations ->
  import_locations_ready locations observations ->
  import_physical_retry_run locations (import_project_cold_observations observations)
    (import_project_cold_observations observations) current 0.
Proof.
  intros locations initial records current observations Trace Ready. constructor.
  intros alias key bytes Member Found.
  eapply import_physical_trace_retains_every_observed_byte; eauto.
Qed.

Theorem import_physical_retries_have_a_strict_finite_budget :
  forall locations initial observed current conflicts,
  import_physical_retry_run locations initial observed current conflicts ->
  conflicts + import_absent_locations locations observed <= import_absent_locations locations initial /\
  import_location_extension locations observed current.
Proof.
  intros locations initial observed current conflicts Run. induction Run as
    [current Extended|observed current conflicts conflicted records refreshed observations Run [Budget Extended]
     Writes Mismatch Trace Ready].
  - split; auto.
  - destruct (import_fresh_reads_after_definite_conflict_strictly_reduce_retry_budget
      _ _ _ _ _ _ _ Extended Writes Mismatch Trace Ready) as [Reduced Preserved].
    split; [lia|exact Preserved].
Qed.

Theorem import_two_alias_retry_attempts_are_bounded_by_initial_absence :
  forall keys initial observed current conflicts,
  import_physical_retry_run (import_retry_locations keys) initial observed current conflicts ->
  S conflicts <= S (import_absent_locations (import_retry_locations keys) initial) /\
  S conflicts <= S (2 * length keys).
Proof.
  intros keys initial observed current conflicts Run.
  destruct (import_physical_retries_have_a_strict_finite_budget _ _ _ _ _ Run) as [Budget _].
  pose proof (import_absence_budget_is_bounded_by_physical_locations (import_retry_locations keys) initial) as Bound.
  rewrite import_two_alias_formats_give_two_locations_per_key in Bound. lia.
Qed.

Theorem import_final_conflict_is_bounded_even_if_refresh_fails :
  forall locations initial observed current conflicts conflicted,
  import_physical_retry_run locations initial observed current conflicts ->
  import_encoded_run current conflicted ->
  import_location_mismatch locations observed conflicted ->
  S conflicts <= import_absent_locations locations initial.
Proof.
  intros locations initial observed current conflicts conflicted Run Writes Mismatch.
  destruct (import_physical_retries_have_a_strict_finite_budget _ _ _ _ _ Run) as [Budget Previous].
  destruct (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings
    (fun _ => None) _ _ Writes) as [Extended _].
  assert (import_location_extension locations observed conflicted) as Combined.
  { intros alias key bytes Member Found. apply Extended. eapply Previous; eauto. }
  pose proof (import_changed_guard_under_extension_consumes_absence _ _ _ Combined Mismatch) as Strict.
  lia.
Qed.

Definition import_location_eq_dec : forall first second : ImportColdLocation,
  {first = second} + {first <> second}.
Proof. decide equality; [apply Nat.eq_dec|apply import_cold_alias_eq_dec]. Defined.

Definition import_write_physical_alias (store : ImportEncodedColdStore)
    (location : ImportColdLocation) (replacement : option (list nat)) : ImportEncodedColdStore :=
  fun alias key => if import_location_eq_dec (alias, key) location then replacement else store alias key.

Lemma import_physical_alias_write_preserves_other_locations : forall store location replacement alias key,
  (alias, key) <> location ->
  import_write_physical_alias store location replacement alias key = store alias key.
Proof.
  intros store location replacement alias key Different. unfold import_write_physical_alias.
  destruct (import_location_eq_dec (alias, key) location); congruence.
Qed.

Fixpoint import_stage_physical_alias_guards (locations : list ImportColdLocation)
    (observed replacements current : ImportEncodedColdStore) : option ImportEncodedColdStore :=
  match locations with
  | [] => Some current
  | (alias, key) :: rest =>
      if import_encoded_bytes_eq_dec (observed alias key) (current alias key) then
        import_stage_physical_alias_guards rest observed replacements
          (import_write_physical_alias current (alias, key) (replacements alias key))
      else None
  end.

Theorem import_normalized_transaction_conflict_requires_an_external_guard_mismatch :
  forall locations observed replacements current,
  NoDup locations ->
  import_stage_physical_alias_guards locations observed replacements current = None ->
  import_location_mismatch locations observed current.
Proof.
  induction locations as [|[alias key] rest IH]; intros observed replacements current Unique Failed.
  - discriminate.
  - inversion Unique as [|location tail Absent UniqueTail]; subst.
    cbn in Failed.
    destruct (import_encoded_bytes_eq_dec (observed alias key) (current alias key)) as [Same|Different].
    + destruct (IH _ _ _ UniqueTail Failed) as [selected [query [Member Mismatch]]].
      exists selected, query. split; [now right|].
      rewrite import_physical_alias_write_preserves_other_locations in Mismatch.
      * exact Mismatch.
      * intros Equal. apply Absent. now rewrite <- Equal.
    + exists alias, key. split; [now left|exact Different].
Qed.

Theorem import_normalized_final_cas_failure_consumes_a_distinct_absence :
  forall locations initial observed current conflicts conflicted replacements,
  NoDup locations ->
  import_physical_retry_run locations initial observed current conflicts ->
  import_encoded_run current conflicted ->
  import_stage_physical_alias_guards locations observed replacements conflicted = None ->
  S conflicts <= import_absent_locations locations initial.
Proof.
  intros locations initial observed current conflicts conflicted replacements Unique Run Writes Failed.
  eapply import_final_conflict_is_bounded_even_if_refresh_fails; eauto.
  eapply import_normalized_transaction_conflict_requires_an_external_guard_mismatch; eauto.
Qed.

Definition import_normalized_retry_locations (keys : list nat) : list ImportColdLocation :=
  nodup import_location_eq_dec (import_retry_locations keys).

Theorem import_normalization_preserves_every_required_physical_guard : forall keys location,
  In location (import_normalized_retry_locations keys) <-> In location (import_retry_locations keys).
Proof. intros keys location. apply nodup_In. Qed.

Theorem import_normalization_produces_one_guard_per_physical_location : forall keys,
  NoDup (import_normalized_retry_locations keys).
Proof. intros keys. apply NoDup_nodup. Qed.

Theorem import_normalization_does_not_increase_the_two_alias_budget : forall keys store,
  import_absent_locations (import_normalized_retry_locations keys) store <= 2 * length keys.
Proof.
  intros keys store.
  pose proof (import_absence_budget_is_bounded_by_physical_locations
    (import_normalized_retry_locations keys) store) as Bound.
  assert (length (import_normalized_retry_locations keys) <= length (import_retry_locations keys)) as Length.
  { apply NoDup_incl_length; [apply import_normalization_produces_one_guard_per_physical_location|].
    intros location Member. now apply import_normalization_preserves_every_required_physical_guard. }
  rewrite import_two_alias_formats_give_two_locations_per_key in Length. lia.
Qed.

Example import_retry_budget_counts_aliases_not_wallets :
  import_absent_locations (import_retry_locations [3; 5; 7]) import_empty_encoded_store = 6.
Proof. reflexivity. Qed.

Example import_present_aliases_reduce_the_initial_retry_budget :
  let store := import_fill_encoded_alias import_empty_encoded_store ImportRawAlias 3 import_valid_empty_joins_bytes in
  import_absent_locations (import_retry_locations [3; 5; 7]) store = 5.
Proof. vm_compute. reflexivity. Qed.

Example import_unchanged_store_has_no_definite_guard_mismatch :
  ~ import_location_mismatch (import_retry_locations [3; 5; 7]) import_empty_encoded_store import_empty_encoded_store.
Proof. intros [alias [key [_ Different]]]. contradiction. Qed.

Example import_overwrite_breaks_the_retry_extension_premise :
  let first := import_fill_encoded_alias import_empty_encoded_store ImportRawAlias 3 [1] in
  let last := import_fill_encoded_alias import_empty_encoded_store ImportRawAlias 3 [2] in
  ~ import_location_extension (import_retry_locations [3]) first last.
Proof.
  intros first last Extended.
  specialize (Extended ImportRawAlias 3 [1] (or_introl eq_refl) eq_refl). discriminate.
Qed.

Example import_duplicate_guards_can_conflict_without_any_external_progress :
  let locations := [(ImportRawAlias, 3); (ImportRawAlias, 3)] in
  let replacements := import_fill_encoded_alias import_empty_encoded_store ImportRawAlias 3 [1] in
  import_stage_physical_alias_guards locations import_empty_encoded_store replacements
    import_empty_encoded_store = None /\
  ~ import_location_mismatch locations import_empty_encoded_store import_empty_encoded_store.
Proof.
  split; [vm_compute; reflexivity|]. intros [alias [key [_ Different]]]. contradiction.
Qed.

Example import_reusing_old_observations_repeats_the_same_conflict :
  let locations := [(ImportRawAlias, 3)] in
  let current := import_fill_encoded_alias import_empty_encoded_store ImportRawAlias 3 [1] in
  import_stage_physical_alias_guards locations import_empty_encoded_store current current = None /\
  match import_stage_physical_alias_guards locations current current current with
  | Some result => result ImportRawAlias 3 = Some [1]
  | None => False
  end.
Proof. vm_compute. split; reflexivity. Qed.
