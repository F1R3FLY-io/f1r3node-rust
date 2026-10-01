From Stdlib Require Import List Arith Bool.
From CostAccountedRho Require Import StateImportClosure.

Import ListNotations.

Definition import_map_extension {A : Type} (old new : nat -> option A) : Prop :=
  forall key value, old key = Some value -> new key = Some value.

Definition import_physical_extension (old new : ImportStore) : Prop :=
  import_map_extension (import_history old) (import_history new) /\
  import_map_extension (import_cold_raw old) (import_cold_raw new) /\
  import_map_extension (import_cold_legacy old) (import_cold_legacy new).

Definition import_alias_agreement (store : ImportStore) : Prop :=
  forall key raw legacy,
    import_cold_raw store key = Some raw ->
    import_cold_legacy store key = Some legacy -> raw = legacy.

Definition import_raw_first_read (store : ImportStore) (key : nat) : option ImportLeaf :=
  match import_cold_raw store key with
  | Some value => Some value
  | None => import_cold_legacy store key
  end.

Definition import_split_alias_read
  (first second : ImportStore) (key : nat) : option ImportLeaf :=
  match import_cold_raw first key with
  | Some value => Some value
  | None => import_cold_legacy second key
  end.

Lemma import_strict_resolution_has_compatible_aliases :
  forall store key value,
    import_resolve_cold store key = Some value ->
    (import_cold_raw store key = Some value \/ import_cold_legacy store key = Some value) /\
    (forall raw, import_cold_raw store key = Some raw -> raw = value) /\
    (forall legacy, import_cold_legacy store key = Some legacy -> legacy = value).
Proof.
  intros store key value resolved. unfold import_resolve_cold in resolved.
  destruct (import_cold_raw store key) as [raw |] eqn:raw_found;
    destruct (import_cold_legacy store key) as [legacy |] eqn:legacy_found;
    try discriminate.
  - destruct (import_leaf_eq_dec raw legacy) as [same | different]; try discriminate.
    inversion resolved. subst. split.
    + left. reflexivity.
    + split; intros; congruence.
  - inversion resolved. subst. split.
    + left. reflexivity.
    + split; intros; congruence.
  - inversion resolved. subst. split.
    + right. reflexivity.
    + split; intros; congruence.
Qed.

Theorem import_strict_resolution_agrees_with_raw_first_reader :
  forall store key value,
    import_resolve_cold store key = Some value ->
    import_raw_first_read store key = Some value.
Proof.
  intros store key value resolved.
  destruct (import_strict_resolution_has_compatible_aliases store key value resolved)
    as [[raw | legacy] [raw_equal legacy_equal]];
    unfold import_raw_first_read.
  - rewrite raw. reflexivity.
  - destruct (import_cold_raw store key) as [found |] eqn:raw.
    + rewrite (raw_equal found eq_refl). reflexivity.
    + exact legacy.
Qed.

Theorem import_physical_extension_preserves_exact_bindings :
  forall old new,
    import_physical_extension old new -> import_alias_agreement new ->
    import_binding_extension old new.
Proof.
  intros old new [history [raw_ext legacy_ext]] agreement. split.
  - exact history.
  - intros key value resolved.
    destruct (import_strict_resolution_has_compatible_aliases old key value resolved)
      as [[raw | legacy] _].
    + pose proof (raw_ext key value raw) as new_raw.
      unfold import_resolve_cold. rewrite new_raw.
      destruct (import_cold_legacy new key) as [other |] eqn:new_legacy.
      * pose proof (agreement key value other new_raw new_legacy) as same.
        subst other. destruct (import_leaf_eq_dec value value); congruence.
      * reflexivity.
    + pose proof (legacy_ext key value legacy) as new_legacy.
      unfold import_resolve_cold. rewrite new_legacy.
      destruct (import_cold_raw new key) as [other |] eqn:new_raw.
      * pose proof (agreement key other value new_raw new_legacy) as same.
        subst other. destruct (import_leaf_eq_dec value value); congruence.
      * reflexivity.
Qed.

Theorem import_two_lookup_reader_preserves_exact_value :
  forall initial first second key value,
    import_resolve_cold initial key = Some value ->
    import_physical_extension initial first ->
    import_physical_extension first second ->
    import_alias_agreement first ->
    import_split_alias_read first second key = Some value.
Proof.
  intros initial first second key value resolved first_ext second_ext agreement.
  pose proof (import_physical_extension_preserves_exact_bindings
    initial first first_ext agreement) as [_ cold_ext].
  pose proof (cold_ext key value resolved) as first_resolved.
  destruct (import_strict_resolution_has_compatible_aliases first key value first_resolved)
    as [[raw | legacy] [raw_equal legacy_equal]];
    unfold import_split_alias_read.
  - rewrite raw. reflexivity.
  - destruct (import_cold_raw first key) as [found |] eqn:raw.
    + rewrite (raw_equal found eq_refl). reflexivity.
    + destruct second_ext as [_ [_ legacy_ext]]. apply legacy_ext. exact legacy.
Qed.

Definition import_update {A : Type}
  (store : nat -> option A) (key : nat) (value : A) : nat -> option A :=
  fun query => if Nat.eq_dec query key then Some value else store query.

Lemma import_update_same : forall A (store : nat -> option A) key value,
  import_update store key value key = Some value.
Proof. intros. unfold import_update. destruct (Nat.eq_dec key key); congruence. Qed.

Lemma import_update_other : forall A (store : nat -> option A) key query value,
  query <> key -> import_update store key value query = store query.
Proof. intros. unfold import_update. destruct (Nat.eq_dec query key); congruence. Qed.

Definition import_absent_or_equal {A : Type}
  (eq_dec : forall left right : A, {left = right} + {left <> right})
  (current : option A) (value : A) : bool :=
  match current with
  | None => true
  | Some old => if eq_dec old value then true else false
  end.

Lemma import_absent_or_equal_preserves_existing :
  forall A eq_dec (current : option A) value old,
    import_absent_or_equal eq_dec current value = true ->
    current = Some old -> old = value.
Proof.
  intros A eq_dec current value old accepted found.
  rewrite found in accepted. unfold import_absent_or_equal in accepted.
  destruct (eq_dec old value); congruence.
Qed.

Lemma import_compatible_update_extends_map :
  forall A eq_dec (store : nat -> option A) key value,
    import_absent_or_equal eq_dec (store key) value = true ->
    import_map_extension store (import_update store key value).
Proof.
  intros A eq_dec store key value compatible query old found.
  destruct (Nat.eq_dec query key) as [same | different].
  - subst query. rewrite import_update_same.
    f_equal. symmetry. eapply import_absent_or_equal_preserves_existing; eauto.
  - rewrite import_update_other by exact different. exact found.
Qed.

Inductive ImportColdAlias : Type := ImportRawAlias | ImportLegacyAlias.

Definition import_cold_insert_candidate
  (store : ImportStore) (alias : ImportColdAlias) (key : nat) (value : ImportLeaf)
  : ImportStore :=
  match alias with
  | ImportRawAlias =>
      {| import_history := import_history store;
         import_cold_raw := import_update (import_cold_raw store) key value;
         import_cold_legacy := import_cold_legacy store |}
  | ImportLegacyAlias =>
      {| import_history := import_history store;
         import_cold_raw := import_cold_raw store;
         import_cold_legacy := import_update (import_cold_legacy store) key value |}
  end.

Definition import_guarded_cold_insert
  (store : ImportStore) (alias : ImportColdAlias) (key : nat) (value : ImportLeaf)
  : option ImportStore :=
  if import_absent_or_equal import_leaf_eq_dec (import_cold_raw store key) value &&
     import_absent_or_equal import_leaf_eq_dec (import_cold_legacy store key) value
  then Some (import_cold_insert_candidate store alias key value) else None.

Theorem import_guarded_cold_insert_preserves_physical_bindings :
  forall store alias key value result,
    import_guarded_cold_insert store alias key value = Some result ->
    import_physical_extension store result.
Proof.
  intros store alias key value result accepted.
  unfold import_guarded_cold_insert in accepted.
  destruct (_ && _) eqn:guard; try discriminate.
  apply andb_true_iff in guard. destruct guard as [raw legacy].
  inversion accepted. subst result.
  destruct alias; unfold import_physical_extension; simpl; split.
  - intros query old found. exact found.
  - split.
    + eapply import_compatible_update_extends_map. exact raw.
    + intros query old found. exact found.
  - intros query old found. exact found.
  - split.
    + intros query old found. exact found.
    + eapply import_compatible_update_extends_map. exact legacy.
Qed.

Theorem import_guarded_cold_insert_preserves_alias_agreement :
  forall store alias key value result,
    import_alias_agreement store ->
    import_guarded_cold_insert store alias key value = Some result ->
    import_alias_agreement result.
Proof.
  intros store alias key value result agreement accepted.
  unfold import_guarded_cold_insert in accepted.
  destruct (_ && _) eqn:guard; try discriminate.
  apply andb_true_iff in guard. destruct guard as [raw_ok legacy_ok].
  inversion accepted. subst result. destruct alias;
    intros query raw legacy raw_found legacy_found; simpl in *;
    destruct (Nat.eq_dec query key) as [same | different].
  - subst query. rewrite import_update_same in raw_found. inversion raw_found. subst raw.
    symmetry. exact (import_absent_or_equal_preserves_existing ImportLeaf import_leaf_eq_dec
      (import_cold_legacy store key) value legacy legacy_ok legacy_found).
  - rewrite import_update_other in raw_found by exact different.
    eapply agreement; eauto.
  - subst query. rewrite import_update_same in legacy_found. inversion legacy_found. subst legacy.
    exact (import_absent_or_equal_preserves_existing ImportLeaf import_leaf_eq_dec
      (import_cold_raw store key) value raw raw_ok raw_found).
  - rewrite import_update_other in legacy_found by exact different.
    eapply agreement; eauto.
Qed.

Definition import_atomic_cold_attempt
  (store : ImportStore) (alias : ImportColdAlias) (key : nat) (value : ImportLeaf)
  (storage_commit_succeeds : bool) : ImportStore * bool :=
  match import_guarded_cold_insert store alias key value with
  | Some candidate => if storage_commit_succeeds then (candidate, true) else (store, false)
  | None => (store, false)
  end.

Theorem import_failed_atomic_attempt_preserves_all_stores :
  forall store alias key value storage_ok result,
    import_atomic_cold_attempt store alias key value storage_ok = (result, false) ->
    result = store.
Proof.
  intros store alias key value storage_ok result attempt.
  unfold import_atomic_cold_attempt in attempt.
  destruct (import_guarded_cold_insert store alias key value);
    destruct storage_ok; inversion attempt; reflexivity.
Qed.

Theorem import_successful_atomic_attempt_preserves_bindings_and_aliases :
  forall store alias key value storage_ok result,
    import_alias_agreement store ->
    import_atomic_cold_attempt store alias key value storage_ok = (result, true) ->
    import_physical_extension store result /\ import_alias_agreement result.
Proof.
  intros store alias key value storage_ok result agreement attempt.
  unfold import_atomic_cold_attempt in attempt.
  destruct (import_guarded_cold_insert store alias key value) as [candidate |] eqn:accepted;
    try discriminate.
  destruct storage_ok; inversion attempt. subst result. split.
  - eapply import_guarded_cold_insert_preserves_physical_bindings; eauto.
  - eapply import_guarded_cold_insert_preserves_alias_agreement; eauto.
Qed.

Theorem import_physical_extension_reflexive :
  forall store, import_physical_extension store store.
Proof. intros store. repeat split; intros key value found; exact found. Qed.

Theorem import_physical_extension_transitive :
  forall first middle last,
    import_physical_extension first middle -> import_physical_extension middle last ->
    import_physical_extension first last.
Proof.
  intros first middle last [h1 [r1 l1]] [h2 [r2 l2]].
  repeat split; intros key value found; auto.
Qed.

Inductive import_physical_run : ImportStore -> ImportStore -> Prop :=
| import_physical_run_refl : forall store, import_physical_run store store
| import_physical_run_commit : forall first middle last,
    import_physical_run first middle ->
    import_physical_extension middle last ->
    import_alias_agreement last ->
    import_physical_run first last.

Theorem import_physical_run_preserves_every_binding :
  forall first last,
    import_physical_run first last -> import_physical_extension first last.
Proof.
  intros first last run. induction run.
  - apply import_physical_extension_reflexive.
  - eapply import_physical_extension_transitive; eauto.
Qed.

Theorem import_physical_run_preserves_alias_agreement :
  forall first last,
    import_alias_agreement first -> import_physical_run first last ->
    import_alias_agreement last.
Proof. intros first last agreement run. induction run; assumption. Qed.

Theorem import_physical_run_preserves_any_number_of_closed_roots :
  forall history_hash payload_hash first last roots,
    import_alias_agreement first ->
    Forall (import_closed history_hash payload_hash first) roots ->
    import_physical_run first last ->
    Forall (import_closed history_hash payload_hash last) roots.
Proof.
  intros history_hash payload_hash first last roots agreement closed run.
  pose proof (import_physical_run_preserves_every_binding first last run) as physical.
  pose proof (import_physical_run_preserves_alias_agreement first last agreement run) as aliases.
  pose proof (import_physical_extension_preserves_exact_bindings first last physical aliases)
    as bindings.
  pose proof (import_binding_extension_preserves_checked_reads
    history_hash payload_hash first last bindings) as checked.
  induction closed as [| root roots root_closed others preserved].
  - constructor.
  - constructor.
    + eapply import_compatible_insertion_preserves_closed_root; eauto.
    + exact preserved.
Qed.

Definition import_radix_target_eq_dec :
  forall left right : ImportRadixTarget, {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition import_radix_edge_eq_dec :
  forall left right : ImportRadixEdge, {left = right} + {left <> right}.
Proof.
  decide equality.
  - apply import_radix_target_eq_dec.
  - apply list_eq_dec. apply Nat.eq_dec.
  - apply Nat.eq_dec.
Defined.

Definition import_guarded_history_insert
  (store : ImportStore) (key : nat) (edges : list ImportRadixEdge) : option ImportStore :=
  if import_absent_or_equal (list_eq_dec import_radix_edge_eq_dec)
    (import_history store key) edges
  then Some
    {| import_history := import_update (import_history store) key edges;
       import_cold_raw := import_cold_raw store;
       import_cold_legacy := import_cold_legacy store |}
  else None.

Theorem import_guarded_history_insert_preserves_bindings_and_aliases :
  forall store key edges result,
    import_alias_agreement store ->
    import_guarded_history_insert store key edges = Some result ->
    import_physical_extension store result /\ import_alias_agreement result.
Proof.
  intros store key edges result agreement accepted.
  unfold import_guarded_history_insert in accepted.
  destruct (import_absent_or_equal _ _ _) eqn:guard; try discriminate.
  inversion accepted. subst result. split.
  - split.
    + simpl. eapply import_compatible_update_extends_map. exact guard.
    + split; intros query value found; exact found.
  - exact agreement.
Qed.

Fixpoint import_guarded_history_batch
  (store : ImportStore) (rows : list (nat * list ImportRadixEdge)) : option ImportStore :=
  match rows with
  | [] => Some store
  | (key, edges) :: rest =>
      match import_guarded_history_insert store key edges with
      | Some candidate => import_guarded_history_batch candidate rest
      | None => None
      end
  end.

Record ImportColdWrite : Type := {
  import_write_alias : ImportColdAlias;
  import_write_key : nat;
  import_write_value : ImportLeaf
}.

Fixpoint import_guarded_cold_batch
  (store : ImportStore) (rows : list ImportColdWrite) : option ImportStore :=
  match rows with
  | [] => Some store
  | row :: rest =>
      match import_guarded_cold_insert store (import_write_alias row)
        (import_write_key row) (import_write_value row) with
      | Some candidate => import_guarded_cold_batch candidate rest
      | None => None
      end
  end.

Theorem import_guarded_history_batch_preserves_bindings_and_aliases :
  forall rows store result,
    import_alias_agreement store ->
    import_guarded_history_batch store rows = Some result ->
    import_physical_extension store result /\ import_alias_agreement result.
Proof.
  intros rows. induction rows as [| [key edges] rest previous];
    intros store result agreement accepted; simpl in accepted.
  - inversion accepted. subst result. split.
    + apply import_physical_extension_reflexive.
    + exact agreement.
  - destruct (import_guarded_history_insert store key edges) as [candidate |] eqn:inserted;
      try discriminate.
    destruct (import_guarded_history_insert_preserves_bindings_and_aliases
      store key edges candidate agreement inserted) as [first_ext candidate_agreement].
    destruct (previous candidate result candidate_agreement accepted) as [last_ext last_agreement].
    split.
    + eapply import_physical_extension_transitive; eauto.
    + exact last_agreement.
Qed.

Theorem import_guarded_cold_batch_preserves_bindings_and_aliases :
  forall rows store result,
    import_alias_agreement store ->
    import_guarded_cold_batch store rows = Some result ->
    import_physical_extension store result /\ import_alias_agreement result.
Proof.
  intros rows. induction rows as [| row rest previous];
    intros store result agreement accepted; simpl in accepted.
  - inversion accepted. subst result. split.
    + apply import_physical_extension_reflexive.
    + exact agreement.
  - destruct (import_guarded_cold_insert store (import_write_alias row)
      (import_write_key row) (import_write_value row)) as [candidate |] eqn:inserted;
      try discriminate.
    pose proof (import_guarded_cold_insert_preserves_physical_bindings
      _ _ _ _ _ inserted) as first_ext.
    pose proof (import_guarded_cold_insert_preserves_alias_agreement
      _ _ _ _ _ agreement inserted) as candidate_agreement.
    destruct (previous candidate result candidate_agreement accepted) as [last_ext last_agreement].
    split.
    + eapply import_physical_extension_transitive; eauto.
    + exact last_agreement.
Qed.

Inductive ImportStoreBatch : Type :=
| ImportHistoryBatch (rows : list (nat * list ImportRadixEdge))
| ImportColdBatch (rows : list ImportColdWrite).

Definition import_stage_batch (store : ImportStore) (batch : ImportStoreBatch) : option ImportStore :=
  match batch with
  | ImportHistoryBatch rows => import_guarded_history_batch store rows
  | ImportColdBatch rows => import_guarded_cold_batch store rows
  end.

Definition import_commit_batch
  (store : ImportStore) (batch : ImportStoreBatch) (storage_ok : bool) : ImportStore * bool :=
  match import_stage_batch store batch with
  | Some candidate => if storage_ok then (candidate, true) else (store, false)
  | None => (store, false)
  end.

Theorem import_failed_batch_does_not_commit_its_staged_prefix :
  forall store batch storage_ok result,
    import_commit_batch store batch storage_ok = (result, false) -> result = store.
Proof.
  intros store batch storage_ok result failed. unfold import_commit_batch in failed.
  destruct (import_stage_batch store batch); destruct storage_ok; inversion failed; reflexivity.
Qed.

Theorem import_every_batch_preserves_bindings_and_aliases :
  forall store batch storage_ok result success,
    import_alias_agreement store ->
    import_commit_batch store batch storage_ok = (result, success) ->
    import_physical_extension store result /\ import_alias_agreement result.
Proof.
  intros store batch storage_ok result success agreement committed.
  destruct success.
  - unfold import_commit_batch in committed.
    destruct (import_stage_batch store batch) as [candidate |] eqn:staged; try discriminate.
    destruct storage_ok; inversion committed. subst result.
    destruct batch as [rows | rows]; simpl in staged.
    + eapply import_guarded_history_batch_preserves_bindings_and_aliases; eauto.
    + eapply import_guarded_cold_batch_preserves_bindings_and_aliases; eauto.
  - pose proof (import_failed_batch_does_not_commit_its_staged_prefix
      _ _ _ _ committed) as same. subst result. split.
    + apply import_physical_extension_reflexive.
    + exact agreement.
Qed.

Inductive import_batch_run : ImportStore -> ImportStore -> Prop :=
| import_batch_run_refl : forall store, import_batch_run store store
| import_batch_run_step : forall first middle last batch storage_ok success,
    import_batch_run first middle ->
    import_commit_batch middle batch storage_ok = (last, success) ->
    import_batch_run first last.

Theorem import_batch_run_refines_physical_run :
  forall first last,
    import_alias_agreement first -> import_batch_run first last ->
    import_physical_run first last.
Proof.
  intros first last agreement run. induction run as
    [store | first middle last batch storage_ok success run previous committed].
  - constructor.
  - specialize (previous agreement).
    pose proof (import_physical_run_preserves_alias_agreement
      first middle agreement previous) as middle_agreement.
    destruct (import_every_batch_preserves_bindings_and_aliases
      _ _ _ _ _ middle_agreement committed) as [extension last_agreement].
    eapply import_physical_run_commit; eauto.
Qed.

Theorem import_arbitrary_batch_interleavings_preserve_closed_roots :
  forall history_hash payload_hash first last roots,
    import_alias_agreement first ->
    Forall (import_closed history_hash payload_hash first) roots ->
    import_batch_run first last ->
    Forall (import_closed history_hash payload_hash last) roots.
Proof.
  intros history_hash payload_hash first last roots agreement closed run.
  eapply import_physical_run_preserves_any_number_of_closed_roots; eauto.
  apply import_batch_run_refines_physical_run; assumption.
Qed.

Theorem import_batch_interleavings_preserve_two_lookup_readers :
  forall initial first second key value,
    import_alias_agreement initial ->
    import_resolve_cold initial key = Some value ->
    import_batch_run initial first -> import_batch_run first second ->
    import_split_alias_read first second key = Some value.
Proof.
  intros initial first second key value agreement resolved run1 run2.
  pose proof (import_batch_run_refines_physical_run initial first agreement run1) as physical1.
  pose proof (import_physical_run_preserves_alias_agreement
    initial first agreement physical1) as agreement1.
  pose proof (import_batch_run_refines_physical_run first second agreement1 run2) as physical2.
  eapply import_two_lookup_reader_preserves_exact_value; eauto;
    apply import_physical_run_preserves_every_binding; assumption.
Qed.

Example import_logical_alias_migration_can_break_a_split_reader :
  let value := {| import_leaf_kind := ImportJoins; import_leaf_payload := [1; 2] |} in
  let raw_only := {| import_history := fun _ => None;
    import_cold_raw := fun _ => Some value; import_cold_legacy := fun _ => None |} in
  let legacy_only := {| import_history := fun _ => None;
    import_cold_raw := fun _ => None; import_cold_legacy := fun _ => Some value |} in
  import_resolve_cold raw_only 3 = Some value /\
  import_resolve_cold legacy_only 3 = Some value /\
  import_binding_extension raw_only legacy_only /\
  import_binding_extension legacy_only raw_only /\
  import_split_alias_read legacy_only raw_only 3 = None.
Proof.
  simpl. repeat split; try reflexivity; intros key value found; try discriminate; exact found.
Qed.

Lemma import_absent_or_equal_cases :
  forall A eq_dec (current : option A) value,
    import_absent_or_equal eq_dec current value = true ->
    current = None \/ current = Some value.
Proof.
  intros A eq_dec current value accepted. destruct current as [old |].
  - right. f_equal. eapply import_absent_or_equal_preserves_existing; eauto.
  - left. reflexivity.
Qed.

Theorem import_guarded_cold_insert_resolves_inserted_value :
  forall store alias key value result,
    import_guarded_cold_insert store alias key value = Some result ->
    import_resolve_cold result key = Some value.
Proof.
  intros store alias key value result accepted.
  unfold import_guarded_cold_insert in accepted.
  destruct (_ && _) eqn:guard; try discriminate.
  apply andb_true_iff in guard. destruct guard as [raw_ok legacy_ok].
  inversion accepted. subst result. destruct alias; unfold import_resolve_cold;
    simpl; rewrite import_update_same.
  - destruct (import_absent_or_equal_cases ImportLeaf import_leaf_eq_dec
      _ _ legacy_ok) as [missing | matching]; rewrite missing || rewrite matching.
    + reflexivity.
    + destruct (import_leaf_eq_dec value value); congruence.
  - destruct (import_absent_or_equal_cases ImportLeaf import_leaf_eq_dec
      _ _ raw_ok) as [missing | matching]; rewrite missing || rewrite matching.
    + reflexivity.
    + destruct (import_leaf_eq_dec value value); congruence.
Qed.

Theorem import_guarded_cold_insert_preserves_exact_bindings_without_global_agreement :
  forall store alias key value result,
    import_guarded_cold_insert store alias key value = Some result ->
    import_binding_extension store result.
Proof.
  intros store alias key value result accepted.
  pose proof (import_guarded_cold_insert_resolves_inserted_value
    _ _ _ _ _ accepted) as resolves.
  pose proof (import_guarded_cold_insert_preserves_physical_bindings
    _ _ _ _ _ accepted) as [history _].
  split.
  - exact history.
  - intros query old resolved. destruct (Nat.eq_dec query key) as [same | different].
    + subst query.
      destruct (import_strict_resolution_has_compatible_aliases store key old resolved)
        as [[raw | legacy] _]; unfold import_guarded_cold_insert in accepted;
        destruct (_ && _) eqn:guard; try discriminate;
        apply andb_true_iff in guard; destruct guard as [raw_ok legacy_ok].
      * pose proof (import_absent_or_equal_preserves_existing ImportLeaf import_leaf_eq_dec
          _ _ _ raw_ok raw) as same_value. subst old. exact resolves.
      * pose proof (import_absent_or_equal_preserves_existing ImportLeaf import_leaf_eq_dec
          _ _ _ legacy_ok legacy) as same_value. subst old. exact resolves.
    + unfold import_guarded_cold_insert in accepted.
      destruct (_ && _); try discriminate. inversion accepted. subst result.
      destruct alias; unfold import_resolve_cold in *; simpl;
        rewrite import_update_other by exact different; exact resolved.
Qed.

Theorem import_guarded_history_insert_preserves_exact_bindings_without_global_agreement :
  forall store key edges result,
    import_guarded_history_insert store key edges = Some result ->
    import_physical_extension store result /\ import_binding_extension store result.
Proof.
  intros store key edges result accepted.
  unfold import_guarded_history_insert in accepted.
  destruct (import_absent_or_equal _ _ _) eqn:guard; try discriminate.
  inversion accepted. subst result.
  assert (history : import_map_extension (import_history store)
    (import_update (import_history store) key edges)).
  { eapply import_compatible_update_extends_map. exact guard. }
  split.
  - split.
    + exact history.
    + split; intros query value found; exact found.
  - split.
    + exact history.
    + intros query value found. exact found.
Qed.

Lemma import_binding_extension_reflexive : forall store, import_binding_extension store store.
Proof. intros store. split; intros key value found; exact found. Qed.

Lemma import_binding_extension_transitive :
  forall first middle last,
    import_binding_extension first middle -> import_binding_extension middle last ->
    import_binding_extension first last.
Proof. intros first middle last [h1 c1] [h2 c2]. split; intros key value found; auto. Qed.

Theorem import_history_batch_preserves_exact_bindings_without_global_agreement :
  forall rows store result,
    import_guarded_history_batch store rows = Some result ->
    import_physical_extension store result /\ import_binding_extension store result.
Proof.
  intros rows. induction rows as [| [key edges] rest previous];
    intros store result accepted; simpl in accepted.
  - inversion accepted. subst result. split.
    + apply import_physical_extension_reflexive.
    + apply import_binding_extension_reflexive.
  - destruct (import_guarded_history_insert store key edges) as [candidate |] eqn:inserted;
      try discriminate.
    destruct (import_guarded_history_insert_preserves_exact_bindings_without_global_agreement
      _ _ _ _ inserted) as [physical1 bindings1].
    destruct (previous candidate result accepted) as [physical2 bindings2].
    split.
    + eapply import_physical_extension_transitive; eauto.
    + eapply import_binding_extension_transitive; eauto.
Qed.

Theorem import_cold_batch_preserves_exact_bindings_without_global_agreement :
  forall rows store result,
    import_guarded_cold_batch store rows = Some result ->
    import_physical_extension store result /\ import_binding_extension store result.
Proof.
  intros rows. induction rows as [| row rest previous];
    intros store result accepted; simpl in accepted.
  - inversion accepted. subst result. split.
    + apply import_physical_extension_reflexive.
    + apply import_binding_extension_reflexive.
  - destruct (import_guarded_cold_insert store (import_write_alias row)
      (import_write_key row) (import_write_value row)) as [candidate |] eqn:inserted;
      try discriminate.
    pose proof (import_guarded_cold_insert_preserves_physical_bindings
      _ _ _ _ _ inserted) as physical1.
    pose proof (import_guarded_cold_insert_preserves_exact_bindings_without_global_agreement
      _ _ _ _ _ inserted) as bindings1.
    destruct (previous candidate result accepted) as [physical2 bindings2].
    split.
    + eapply import_physical_extension_transitive; eauto.
    + eapply import_binding_extension_transitive; eauto.
Qed.

Theorem import_every_batch_preserves_exact_bindings_without_global_agreement :
  forall store batch storage_ok result success,
    import_commit_batch store batch storage_ok = (result, success) ->
    import_physical_extension store result /\ import_binding_extension store result.
Proof.
  intros store batch storage_ok result success committed. destruct success.
  - unfold import_commit_batch in committed.
    destruct (import_stage_batch store batch) as [candidate |] eqn:staged; try discriminate.
    destruct storage_ok; inversion committed. subst result.
    destruct batch as [rows | rows]; simpl in staged.
    + eapply import_history_batch_preserves_exact_bindings_without_global_agreement; eauto.
    + eapply import_cold_batch_preserves_exact_bindings_without_global_agreement; eauto.
  - pose proof (import_failed_batch_does_not_commit_its_staged_prefix
      _ _ _ _ committed) as same. subst result. split.
    + apply import_physical_extension_reflexive.
    + apply import_binding_extension_reflexive.
Qed.

Theorem import_arbitrary_batches_preserve_exact_bindings_without_global_agreement :
  forall first last,
    import_batch_run first last ->
    import_physical_extension first last /\ import_binding_extension first last.
Proof.
  intros first last run. induction run as
    [store | first middle last batch storage_ok success run previous committed].
  - split.
    + apply import_physical_extension_reflexive.
    + apply import_binding_extension_reflexive.
  - destruct previous as [physical1 bindings1].
    destruct (import_every_batch_preserves_exact_bindings_without_global_agreement
      _ _ _ _ _ committed) as [physical2 bindings2].
    split.
    + eapply import_physical_extension_transitive; eauto.
    + eapply import_binding_extension_transitive; eauto.
Qed.

Theorem import_arbitrary_batches_preserve_closed_roots_without_global_agreement :
  forall history_hash payload_hash first last roots,
    Forall (import_closed history_hash payload_hash first) roots ->
    import_batch_run first last ->
    Forall (import_closed history_hash payload_hash last) roots.
Proof.
  intros history_hash payload_hash first last roots closed run.
  destruct (import_arbitrary_batches_preserve_exact_bindings_without_global_agreement
    first last run) as [_ bindings].
  pose proof (import_binding_extension_preserves_checked_reads
    history_hash payload_hash first last bindings) as checked.
  induction closed as [| root roots root_closed others preserved].
  - constructor.
  - constructor.
    + eapply import_compatible_insertion_preserves_closed_root; eauto.
    + exact preserved.
Qed.

Theorem import_arbitrary_batches_preserve_split_readers_without_global_agreement :
  forall initial first second key value,
    import_resolve_cold initial key = Some value ->
    import_batch_run initial first -> import_batch_run first second ->
    import_split_alias_read first second key = Some value.
Proof.
  intros initial first second key value resolved run1 run2.
  destruct (import_arbitrary_batches_preserve_exact_bindings_without_global_agreement
    initial first run1) as [_ [_ cold1]].
  destruct (import_arbitrary_batches_preserve_exact_bindings_without_global_agreement
    first second run2) as [[_ [_ legacy2]] _].
  pose proof (cold1 key value resolved) as first_resolved.
  destruct (import_strict_resolution_has_compatible_aliases first key value first_resolved)
    as [[raw | legacy] [raw_equal _]]; unfold import_split_alias_read.
  - rewrite raw. reflexivity.
  - destruct (import_cold_raw first key) as [found |] eqn:raw.
    + rewrite (raw_equal found eq_refl). reflexivity.
    + apply legacy2. exact legacy.
Qed.
