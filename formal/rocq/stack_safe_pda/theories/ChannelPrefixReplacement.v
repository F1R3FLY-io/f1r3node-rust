From Stdlib Require Import Bool List Arith Lia FunctionalExtensionality.

Import ListNotations.
Set Implicit Arguments.

(** Formal-first contract for [RSpaceDataStore::set_channel].

    The implementation representation is a private PathMap-backed Bag of
    channel-relative suffixes.  Lists in this file are finite-support proof
    witnesses only; they do not prescribe a Vec conversion.  Publication is
    one total prefix replacement after every fallible encoder, iterator,
    multiplicity, capacity, allocation, and cache-lock operation has completed
    privately. *)
Module ChannelPrefixReplacement.

Definition byte := nat.
Definition path := list byte.

Definition path_eq_dec : forall left right : path, {left = right} + {left <> right} :=
  list_eq_dec Nat.eq_dec.

Fixpoint strip_prefix (prefix whole : path) : option path :=
  match prefix, whole with
  | [], suffix => Some suffix
  | _, [] => None
  | expected :: prefix_rest, actual :: whole_rest =>
      if Nat.eqb expected actual
      then strip_prefix prefix_rest whole_rest
      else None
  end.

Theorem strip_prefix_composed_path : forall prefix suffix,
  strip_prefix prefix (prefix ++ suffix) = Some suffix.
Proof.
  induction prefix as [|head rest IH]; intro suffix; simpl.
  - reflexivity.
  - now rewrite Nat.eqb_refl.
Qed.

Theorem successful_strip_prefix_decomposes_path : forall prefix whole suffix,
  strip_prefix prefix whole = Some suffix -> whole = prefix ++ suffix.
Proof.
  induction prefix as [|expected prefix_rest IH]; intros whole suffix stripped.
  - simpl in stripped. now inversion stripped.
  - destruct whole as [|actual whole_rest]; simpl in stripped; try discriminate.
    destruct (Nat.eqb expected actual) eqn:equal; try discriminate.
    apply Nat.eqb_eq in equal. subst actual. simpl. f_equal.
    now eapply IH.
Qed.

Record bag_entry : Type := BagEntry {
  entry_path : path;
  entry_multiplicity : nat
}.

Definition entry_key (entry : bag_entry) : path := entry_path entry.

Definition entries_well_formed (entries : list bag_entry) : Prop :=
  NoDup (map entry_key entries) /\
  Forall (fun entry => entry_multiplicity entry > 0) entries.

(** Production multiplicities are checked [u64] values.  [nat] is the proof
    carrier only; this predicate is the refinement boundary used before a
    prepared bag can be published. *)
Definition max_u64 : nat := 18446744073709551615.

Definition entries_fit_u64 (entries : list bag_entry) : Prop :=
  Forall (fun entry => entry_multiplicity entry <= max_u64) entries.

Fixpoint prefix_projection (prefix : path) (entries : list bag_entry)
    : list bag_entry :=
  match entries with
  | [] => []
  | entry :: rest =>
      match strip_prefix prefix (entry_path entry) with
      | Some suffix =>
          BagEntry suffix (entry_multiplicity entry) ::
          prefix_projection prefix rest
      | None => prefix_projection prefix rest
      end
  end.

Fixpoint keep_outside_prefix (prefix : path) (entries : list bag_entry)
    : list bag_entry :=
  match entries with
  | [] => []
  | entry :: rest =>
      match strip_prefix prefix (entry_path entry) with
      | Some _ => keep_outside_prefix prefix rest
      | None => entry :: keep_outside_prefix prefix rest
      end
  end.

Definition lift_suffix_entry (prefix : path) (entry : bag_entry) : bag_entry :=
  BagEntry (prefix ++ entry_path entry) (entry_multiplicity entry).

Definition lift_suffix_entries (prefix : path) (entries : list bag_entry)
    : list bag_entry := map (lift_suffix_entry prefix) entries.

Definition replace_prefix
    (before : list bag_entry) (prefix : path) (replacement : list bag_entry)
    : list bag_entry :=
  keep_outside_prefix prefix before ++ lift_suffix_entries prefix replacement.

Lemma in_keep_outside_prefix_implies_original : forall prefix entry entries,
  In entry (keep_outside_prefix prefix entries) -> In entry entries.
Proof.
  intros prefix entry entries. induction entries as [|head rest IH]; simpl.
  - tauto.
  - destruct (strip_prefix prefix (entry_path head)); simpl; intros present.
    + right. now apply IH.
    + destruct present as [same|present]; [now left|right; now apply IH].
Qed.

Lemma in_keep_outside_prefix_has_no_target_prefix : forall prefix entry entries,
  In entry (keep_outside_prefix prefix entries) ->
  strip_prefix prefix (entry_path entry) = None.
Proof.
  intros prefix entry entries. induction entries as [|head rest IH]; simpl.
  - tauto.
  - destruct (strip_prefix prefix (entry_path head)) eqn:headed; simpl.
    + now apply IH.
    + intros [same|present].
      * now subst.
      * now apply IH.
Qed.

Lemma keep_outside_prefix_preserves_key_nodup : forall prefix entries,
  NoDup (map entry_key entries) ->
  NoDup (map entry_key (keep_outside_prefix prefix entries)).
Proof.
  intros prefix entries. induction entries as [|head rest IH]; simpl; intro unique.
  - constructor.
  - inversion unique as [|key keys absent rest_unique]; subst.
    destruct (strip_prefix prefix (entry_path head)); simpl.
    + now apply IH.
    + constructor.
      * intro present. apply absent.
        apply in_map_iff in present as [entry [same present]].
        apply in_map_iff. exists entry. split; [exact same|].
        now apply in_keep_outside_prefix_implies_original in present.
      * now apply IH.
Qed.

Lemma lift_suffix_entries_preserves_key_nodup : forall prefix entries,
  NoDup (map entry_key entries) ->
  NoDup (map entry_key (lift_suffix_entries prefix entries)).
Proof.
  intros prefix entries. induction entries as [|head rest IH]; simpl; intro unique.
  - constructor.
  - inversion unique as [|key keys absent rest_unique]; subst. constructor.
    + intro present. apply absent.
      apply in_map_iff in present as [lifted [same present]].
      apply in_map_iff in present as [original [lifted_is_original original_in]].
      subst lifted.
      apply in_map_iff. exists original. split; [|exact original_in].
      unfold entry_key, lift_suffix_entry in same |- *. simpl in same |- *.
      now apply app_inv_head in same.
    + now apply IH.
Qed.

Lemma kept_and_lifted_keys_are_disjoint : forall prefix before replacement key,
  In key (map entry_key (keep_outside_prefix prefix before)) ->
  ~ In key (map entry_key (lift_suffix_entries prefix replacement)).
Proof.
  intros prefix before replacement key kept lifted.
  apply in_map_iff in kept as [old [old_key old_kept]].
  apply in_map_iff in lifted as [fresh [fresh_key fresh_lifted]].
  apply in_map_iff in fresh_lifted as [original [fresh_is_lift original_in]].
  subst fresh.
  pose proof (in_keep_outside_prefix_has_no_target_prefix
    prefix old before old_kept) as outside.
  unfold entry_key, lift_suffix_entry in old_key, fresh_key. simpl in fresh_key.
  rewrite old_key, <- fresh_key, strip_prefix_composed_path in outside. discriminate.
Qed.

Lemma nodup_append_from_disjoint : forall (A : Type) (left right : list A),
  NoDup left -> NoDup right ->
  (forall value, In value left -> ~ In value right) ->
  NoDup (left ++ right).
Proof.
  intros A left right left_unique right_unique disjoint.
  induction left as [|head tail IH]; simpl.
  - exact right_unique.
  - inversion left_unique as [|value values absent tail_unique]; subst.
    constructor.
    + intro present. apply in_app_iff in present as [in_tail|in_right].
      * contradiction.
      * exact (disjoint head (or_introl eq_refl) in_right).
    + apply IH; [exact tail_unique|].
      intros value in_tail. apply disjoint. now right.
Qed.

Lemma keep_outside_prefix_preserves_positive : forall prefix entries,
  Forall (fun entry => entry_multiplicity entry > 0) entries ->
  Forall (fun entry => entry_multiplicity entry > 0)
    (keep_outside_prefix prefix entries).
Proof.
  intros prefix entries. induction entries as [|head rest IH]; simpl; intro positive.
  - constructor.
  - inversion positive; subst.
    destruct (strip_prefix prefix (entry_path head)).
    + now apply IH.
    + constructor; [assumption|now apply IH].
Qed.

Lemma lift_suffix_entries_preserves_positive : forall prefix entries,
  Forall (fun entry => entry_multiplicity entry > 0) entries ->
  Forall (fun entry => entry_multiplicity entry > 0)
    (lift_suffix_entries prefix entries).
Proof.
  intros prefix entries. induction entries as [|[key count] rest IH]; simpl;
    intro positive.
  - constructor.
  - inversion positive as [|entry entries head_positive rest_positive]; subst.
    constructor; [exact head_positive|now apply IH].
Qed.

Lemma keep_outside_prefix_preserves_u64 : forall prefix entries,
  entries_fit_u64 entries -> entries_fit_u64 (keep_outside_prefix prefix entries).
Proof.
  intros prefix entries. induction entries as [|head rest IH]; simpl; intro bounded.
  - constructor.
  - inversion bounded; subst.
    destruct (strip_prefix prefix (entry_path head)).
    + now apply IH.
    + constructor; [assumption|now apply IH].
Qed.

Lemma lift_suffix_entries_preserves_u64 : forall prefix entries,
  entries_fit_u64 entries -> entries_fit_u64 (lift_suffix_entries prefix entries).
Proof.
  intros prefix entries. induction entries as [|[key count] rest IH]; simpl;
    intro bounded.
  - constructor.
  - inversion bounded as [|entry entries head_bounded rest_bounded]; subst.
    constructor; [exact head_bounded|now apply IH].
Qed.

Theorem replace_prefix_preserves_well_formedness : forall before prefix replacement,
  entries_well_formed before ->
  entries_well_formed replacement ->
  entries_well_formed (replace_prefix before prefix replacement).
Proof.
  intros before prefix replacement [before_unique before_positive]
    [replacement_unique replacement_positive].
  unfold replace_prefix, entries_well_formed. split.
  - rewrite map_app. apply nodup_append_from_disjoint.
    + now apply keep_outside_prefix_preserves_key_nodup.
    + now apply lift_suffix_entries_preserves_key_nodup.
    + exact (kept_and_lifted_keys_are_disjoint prefix before replacement).
  - apply Forall_app. split.
    + now apply keep_outside_prefix_preserves_positive.
    + now apply lift_suffix_entries_preserves_positive.
Qed.

Theorem replace_prefix_preserves_u64_bound : forall before prefix replacement,
  entries_fit_u64 before -> entries_fit_u64 replacement ->
  entries_fit_u64 (replace_prefix before prefix replacement).
Proof.
  intros before prefix replacement before_bounded replacement_bounded.
  unfold replace_prefix. apply Forall_app. split.
  - now apply keep_outside_prefix_preserves_u64.
  - now apply lift_suffix_entries_preserves_u64.
Qed.

Theorem prefix_projection_append : forall prefix left right,
  prefix_projection prefix (left ++ right) =
  prefix_projection prefix left ++ prefix_projection prefix right.
Proof.
  intros prefix left. induction left as [|entry rest IH]; intro right; simpl.
  - reflexivity.
  - destruct (strip_prefix prefix (entry_path entry)); simpl; now rewrite IH.
Qed.

Theorem target_projection_of_kept_entries_is_empty : forall prefix entries,
  prefix_projection prefix (keep_outside_prefix prefix entries) = [].
Proof.
  intros prefix entries. induction entries as [|entry rest IH]; simpl.
  - reflexivity.
  - destruct (strip_prefix prefix (entry_path entry)) eqn:stripped; simpl.
    + exact IH.
    + rewrite stripped. exact IH.
Qed.

Theorem target_projection_of_lifted_replacement_is_exact : forall prefix replacement,
  prefix_projection prefix (lift_suffix_entries prefix replacement) = replacement.
Proof.
  intros prefix replacement. induction replacement as [|[entry count] rest IH]; simpl.
  - reflexivity.
  - rewrite strip_prefix_composed_path. simpl. now rewrite IH.
Qed.

Theorem successful_publication_exactly_replaces_target_prefix : forall
    before prefix replacement,
  prefix_projection prefix (replace_prefix before prefix replacement) = replacement.
Proof.
  intros. unfold replace_prefix. rewrite prefix_projection_append.
  rewrite target_projection_of_kept_entries_is_empty.
  rewrite target_projection_of_lifted_replacement_is_exact. reflexivity.
Qed.

Fixpoint lookup_multiplicity (selected : path) (entries : list bag_entry)
    : option nat :=
  match entries with
  | [] => None
  | entry :: rest =>
      if path_eq_dec (entry_path entry) selected
      then Some (entry_multiplicity entry)
      else lookup_multiplicity selected rest
  end.

(** Unlike [lookup_multiplicity], this fold intentionally admits repeated
    source datum identities.  PREPARE must aggregate them into one checked
    suffix endpoint without changing their total multiplicity. *)
Fixpoint total_multiplicity_at (selected : path) (entries : list bag_entry)
    : nat :=
  match entries with
  | [] => 0
  | entry :: rest =>
      (if path_eq_dec (entry_path entry) selected
       then entry_multiplicity entry else 0) +
      total_multiplicity_at selected rest
  end.

Theorem lookup_multiplicity_append : forall selected left right,
  lookup_multiplicity selected (left ++ right) =
  match lookup_multiplicity selected left with
  | Some count => Some count
  | None => lookup_multiplicity selected right
  end.
Proof.
  intros selected left. induction left as [|entry rest IH]; intro right; simpl.
  - reflexivity.
  - destruct (path_eq_dec (entry_path entry) selected); [reflexivity|apply IH].
Qed.

Theorem outside_lookup_survives_prefix_filter : forall
    selected prefix before,
  strip_prefix prefix selected = None ->
  lookup_multiplicity selected (keep_outside_prefix prefix before) =
  lookup_multiplicity selected before.
Proof.
  intros selected prefix before. induction before as [|entry rest IH];
    intro outside; simpl.
  - reflexivity.
  - destruct (strip_prefix prefix (entry_path entry)) eqn:entry_prefix.
    + destruct (path_eq_dec (entry_path entry) selected) as [same|different].
      * subst. rewrite outside in entry_prefix. discriminate.
      * simpl. destruct (path_eq_dec (entry_path entry) selected) as [same|different'];
          [contradiction|apply IH; assumption].
    + simpl. destruct (path_eq_dec (entry_path entry) selected).
      * reflexivity.
      * apply IH. assumption.
Qed.

Theorem outside_lookup_absent_from_lifted_replacement : forall
    selected prefix replacement,
  strip_prefix prefix selected = None ->
  lookup_multiplicity selected (lift_suffix_entries prefix replacement) = None.
Proof.
  intros selected prefix replacement outside.
  induction replacement as [|entry rest IH]; simpl.
  - reflexivity.
  - destruct (path_eq_dec (prefix ++ entry_path entry) selected) as [same|different].
    + subst. rewrite strip_prefix_composed_path in outside. discriminate.
    + exact IH.
Qed.

Theorem successful_publication_preserves_every_non_target_key : forall
    before prefix replacement selected,
  strip_prefix prefix selected = None ->
  lookup_multiplicity selected (replace_prefix before prefix replacement) =
  lookup_multiplicity selected before.
Proof.
  intros before prefix replacement selected outside.
  unfold replace_prefix. rewrite lookup_multiplicity_append.
  rewrite outside_lookup_survives_prefix_filter by exact outside.
  destruct (lookup_multiplicity selected before); [reflexivity|].
  now apply outside_lookup_absent_from_lifted_replacement.
Qed.

Definition prefix_independent (left right : path) : Prop :=
  (forall suffix, strip_prefix left (right ++ suffix) = None) /\
  (forall suffix, strip_prefix right (left ++ suffix) = None).

Theorem independent_projection_of_kept_entries_is_unchanged : forall
    target other before,
  prefix_independent target other ->
  prefix_projection other (keep_outside_prefix target before) =
  prefix_projection other before.
Proof.
  intros target other before independent.
  destruct independent as [_ target_not_other].
  induction before as [|entry rest IH]; simpl.
  - reflexivity.
  - destruct (strip_prefix target (entry_path entry)) as [target_suffix|]
      eqn:under_target.
    + assert (entry_path entry = target ++ target_suffix) as decomposed.
      { eapply successful_strip_prefix_decomposes_path. exact under_target. }
      rewrite decomposed. rewrite target_not_other. exact IH.
    + destruct (strip_prefix other (entry_path entry)) eqn:under_other; simpl;
        rewrite under_other; rewrite IH; reflexivity.
Qed.

Theorem independent_projection_of_lifted_target_is_empty : forall
    target other replacement,
  prefix_independent target other ->
  prefix_projection other (lift_suffix_entries target replacement) = [].
Proof.
  intros target other replacement independent.
  destruct independent as [_ target_not_other].
  induction replacement as [|entry rest IH]; simpl.
  - reflexivity.
  - rewrite target_not_other. exact IH.
Qed.

Theorem successful_publication_preserves_independent_prefix : forall
    before target other replacement,
  prefix_independent target other ->
  prefix_projection other (replace_prefix before target replacement) =
  prefix_projection other before.
Proof.
  intros. unfold replace_prefix. rewrite prefix_projection_append.
  rewrite independent_projection_of_kept_entries_is_unchanged by exact H.
  rewrite independent_projection_of_lifted_target_is_empty by exact H.
  now rewrite app_nil_r.
Qed.

Definition distinct_count (entries : list bag_entry) : nat := length entries.

Fixpoint cardinality (entries : list bag_entry) : nat :=
  match entries with
  | [] => 0
  | entry :: rest => entry_multiplicity entry + cardinality rest
  end.

Record staged_bag_refines_input
    (source staged : list bag_entry) : Prop := StagedBagRefinesInput {
  staged_pointwise_multiplicity : forall suffix,
    total_multiplicity_at suffix staged = total_multiplicity_at suffix source;
  staged_total_cardinality : cardinality staged = cardinality source
}.

Theorem cardinality_append : forall left right,
  cardinality (left ++ right) = cardinality left + cardinality right.
Proof.
  intros left. induction left as [|[entry count] rest IH]; intro right; simpl.
  - reflexivity.
  - rewrite IH. lia.
Qed.

Theorem cardinality_lift_suffix_entries : forall prefix replacement,
  cardinality (lift_suffix_entries prefix replacement) = cardinality replacement.
Proof.
  intros prefix replacement.
  induction replacement as [|[entry count] rest IH]; simpl; lia.
Qed.

Theorem distinct_partition_at_prefix : forall prefix before,
  distinct_count before =
    distinct_count (prefix_projection prefix before) +
    distinct_count (keep_outside_prefix prefix before).
Proof.
  intros prefix before. induction before as [|[entry count] rest IH]; simpl.
  - reflexivity.
  - destruct (strip_prefix prefix entry); simpl in *; lia.
Qed.

Theorem cardinality_partition_at_prefix : forall prefix before,
  cardinality before =
    cardinality (prefix_projection prefix before) +
    cardinality (keep_outside_prefix prefix before).
Proof.
  intros prefix before. induction before as [|[entry count] rest IH]; simpl.
  - reflexivity.
  - destruct (strip_prefix prefix entry); simpl in *; lia.
Qed.

Theorem successful_publication_distinct_count_formula : forall
    before prefix replacement,
  distinct_count (replace_prefix before prefix replacement) =
  distinct_count before - distinct_count (prefix_projection prefix before) +
  distinct_count replacement.
Proof.
  intros. unfold replace_prefix, distinct_count, lift_suffix_entries.
  rewrite length_app, length_map.
  pose proof (distinct_partition_at_prefix prefix before). unfold distinct_count in *.
  lia.
Qed.

Theorem successful_publication_cardinality_formula : forall
    before prefix replacement,
  cardinality (replace_prefix before prefix replacement) =
  cardinality before - cardinality (prefix_projection prefix before) +
  cardinality replacement.
Proof.
  intros. unfold replace_prefix. rewrite cardinality_append.
  rewrite cardinality_lift_suffix_entries.
  pose proof (cardinality_partition_at_prefix prefix before). lia.
Qed.

Section Transaction.

Context {Catalog ExternalSnapshots : Type}.
Variable catalog_matches : Catalog -> list bag_entry -> Prop.
Variable replacement_lexicographic : list bag_entry -> Prop.
Variable max_loaded_channels : nat.

Record prepared_suffix_bag : Type := PreparedSuffixBag {
  prepared_entries : list bag_entry;
  prepared_entries_well_formed : entries_well_formed prepared_entries;
  prepared_entries_fit_u64 : entries_fit_u64 prepared_entries;
  prepared_entries_lexicographic : replacement_lexicographic prepared_entries
}.

Record store_state : Type := StoreState {
  authoritative_entries : list bag_entry;
  loaded_markers : list path;
  loaded_marker_count : nat;
  candidate_catalogs : path -> option Catalog;
  external_catalog_snapshots : ExternalSnapshots
}.

Definition marker_loaded (marker : path) (state : store_state) : Prop :=
  In marker (loaded_markers state).

Definition mark_loaded (marker : path) (loaded : list path) : list path :=
  if in_dec path_eq_dec marker loaded then loaded else marker :: loaded.

Definition remove_target_catalog
    (marker : path) (catalogs : path -> option Catalog)
    : path -> option Catalog :=
  fun candidate => if path_eq_dec candidate marker then None else catalogs candidate.

Definition loaded_successor_safe (state : store_state) (marker : path) : Prop :=
  marker_loaded marker state \/ loaded_marker_count state < max_loaded_channels.

Definition publish_prepared_prefix
    (before : store_state) (marker : path) (prepared : prepared_suffix_bag)
    : store_state :=
  let loaded := mark_loaded marker (loaded_markers before) in
  StoreState
    (replace_prefix (authoritative_entries before) marker
      (prepared_entries prepared))
    loaded
    (length loaded)
    (remove_target_catalog marker (candidate_catalogs before))
    (external_catalog_snapshots before).

Inductive preparation_failure : Type :=
| DatumEncodingFailure
| DatumIteratorFailure
| MultiplicityOverflowFailure
| LoadedCountOverflowFailure
| AllocationFailure
| CatalogLockPreflightFailure
| PreparationPanic.

Inductive channel_preparation (before : store_state) (marker : path) : Type :=
| PrepareFailed : preparation_failure -> channel_preparation before marker
| PrepareReady : forall prepared : prepared_suffix_bag,
    loaded_successor_safe before marker -> channel_preparation before marker.

Inductive channel_decision : Type := ChannelSetFailed | ChannelSetPublished.

Record channel_outcome : Type := ChannelOutcome {
  channel_outcome_decision : channel_decision;
  channel_outcome_state : store_state
}.

Inductive set_channel_transition
    (before : store_state) (marker : path)
    : channel_preparation before marker -> channel_outcome -> Prop :=
| SetChannelPreparationFailed : forall failure,
    @set_channel_transition before marker (@PrepareFailed before marker failure)
      (ChannelOutcome ChannelSetFailed before)
| SetChannelPublish : forall prepared count_safe,
    @set_channel_transition before marker
      (@PrepareReady before marker prepared count_safe)
      (ChannelOutcome ChannelSetPublished
        (publish_prepared_prefix before marker prepared)).

Theorem every_preparation_failure_preserves_the_entire_public_state : forall
    before marker preparation outcome,
  @set_channel_transition before marker preparation outcome ->
  channel_outcome_decision outcome = ChannelSetFailed ->
  channel_outcome_state outcome = before.
Proof.
  intros before marker preparation outcome transitioned failed.
  inversion transitioned; subst; simpl in failed; try discriminate; reflexivity.
Qed.

Theorem every_preparation_failure_has_precedence_over_publication : forall
    before marker failure outcome,
  @set_channel_transition before marker
    (@PrepareFailed before marker failure) outcome ->
  outcome = ChannelOutcome ChannelSetFailed before.
Proof. intros. now inversion H. Qed.

Theorem encoding_failure_preserves_bag_loaded_count_and_catalog_identity : forall
    before marker outcome,
  @set_channel_transition before marker
    (@PrepareFailed before marker DatumEncodingFailure) outcome ->
  outcome = ChannelOutcome ChannelSetFailed before.
Proof. intros. inversion H. reflexivity. Qed.

Theorem preparation_panic_is_strongly_atomic : forall before marker outcome,
  @set_channel_transition before marker
    (@PrepareFailed before marker PreparationPanic) outcome ->
  outcome = ChannelOutcome ChannelSetFailed before.
Proof. intros. inversion H. reflexivity. Qed.

Theorem published_target_prefix_is_exact : forall before marker prepared,
  prefix_projection marker
    (authoritative_entries (publish_prepared_prefix before marker prepared)) =
  prepared_entries prepared.
Proof. intros. apply successful_publication_exactly_replaces_target_prefix. Qed.

Theorem published_target_multiplicity_is_exact : forall
    before marker prepared suffix,
  lookup_multiplicity suffix
    (prefix_projection marker
      (authoritative_entries (publish_prepared_prefix before marker prepared))) =
  lookup_multiplicity suffix (prepared_entries prepared).
Proof. intros. now rewrite published_target_prefix_is_exact. Qed.

Theorem publication_preserves_prepared_input_aggregation : forall
    before marker prepared source_entries,
  staged_bag_refines_input source_entries (prepared_entries prepared) ->
  (forall suffix,
    total_multiplicity_at suffix
      (prefix_projection marker
        (authoritative_entries (publish_prepared_prefix before marker prepared))) =
    total_multiplicity_at suffix source_entries) /\
  cardinality
    (prefix_projection marker
      (authoritative_entries (publish_prepared_prefix before marker prepared))) =
  cardinality source_entries.
Proof.
  intros before marker prepared source_entries refinement.
  destruct refinement as [pointwise total].
  rewrite published_target_prefix_is_exact. split; assumption.
Qed.

Theorem publication_preserves_authoritative_well_formedness : forall
    before marker prepared,
  entries_well_formed (authoritative_entries before) ->
  entries_well_formed
    (authoritative_entries (publish_prepared_prefix before marker prepared)).
Proof.
  intros. simpl. apply replace_prefix_preserves_well_formedness; auto.
  apply prepared_entries_well_formed.
Qed.

Theorem publication_preserves_checked_u64_multiplicities : forall
    before marker prepared,
  entries_fit_u64 (authoritative_entries before) ->
  entries_fit_u64
    (authoritative_entries (publish_prepared_prefix before marker prepared)).
Proof.
  intros. simpl. apply replace_prefix_preserves_u64_bound; auto.
  apply prepared_entries_fit_u64.
Qed.

Theorem published_non_target_key_is_preserved : forall
    before marker prepared selected,
  strip_prefix marker selected = None ->
  lookup_multiplicity selected
    (authoritative_entries (publish_prepared_prefix before marker prepared)) =
  lookup_multiplicity selected (authoritative_entries before).
Proof. intros. now apply successful_publication_preserves_every_non_target_key.
Qed.

Theorem published_independent_channel_is_preserved : forall
    before marker other prepared,
  prefix_independent marker other ->
  prefix_projection other
    (authoritative_entries (publish_prepared_prefix before marker prepared)) =
  prefix_projection other (authoritative_entries before).
Proof. intros. now apply successful_publication_preserves_independent_prefix.
Qed.

Theorem target_is_loaded_even_when_replacement_is_empty : forall
    before marker prepared,
  In marker (loaded_markers (publish_prepared_prefix before marker prepared)).
Proof.
  intros. unfold publish_prepared_prefix, mark_loaded. simpl.
  destruct (in_dec path_eq_dec marker (loaded_markers before)); simpl; auto.
Qed.

Theorem already_loaded_replacement_does_not_change_loaded_count : forall
    before marker prepared,
  loaded_marker_count before = length (loaded_markers before) ->
  In marker (loaded_markers before) ->
  loaded_marker_count (publish_prepared_prefix before marker prepared) =
  loaded_marker_count before.
Proof.
  intros before marker prepared count_correct already_loaded.
  unfold publish_prepared_prefix, mark_loaded. simpl.
  destruct (in_dec path_eq_dec marker (loaded_markers before)); [congruence|contradiction].
Qed.

Theorem newly_loaded_replacement_increments_loaded_count_once : forall
    before marker prepared,
  loaded_marker_count before = length (loaded_markers before) ->
  ~ In marker (loaded_markers before) ->
  loaded_marker_count (publish_prepared_prefix before marker prepared) =
  S (loaded_marker_count before).
Proof.
  intros before marker prepared count_correct not_loaded.
  unfold publish_prepared_prefix, mark_loaded. simpl.
  destruct (in_dec path_eq_dec marker (loaded_markers before)); [contradiction|].
  simpl. lia.
Qed.

Theorem publication_loaded_count_matches_loaded_set : forall before marker prepared,
  loaded_marker_count (publish_prepared_prefix before marker prepared) =
  length (loaded_markers (publish_prepared_prefix before marker prepared)).
Proof. reflexivity. Qed.

Lemma mark_loaded_preserves_nodup : forall marker loaded,
  NoDup loaded -> NoDup (mark_loaded marker loaded).
Proof.
  intros marker loaded unique. unfold mark_loaded.
  destruct (in_dec path_eq_dec marker loaded); [exact unique|].
  now constructor.
Qed.

Theorem publication_preserves_loaded_set_uniqueness : forall before marker prepared,
  NoDup (loaded_markers before) ->
  NoDup (loaded_markers (publish_prepared_prefix before marker prepared)).
Proof.
  intros. simpl. now apply mark_loaded_preserves_nodup.
Qed.

Theorem publication_respects_preflighted_loaded_capacity : forall
    before marker prepared,
  loaded_marker_count before = length (loaded_markers before) ->
  loaded_marker_count before <= max_loaded_channels ->
  loaded_successor_safe before marker ->
  loaded_marker_count (publish_prepared_prefix before marker prepared) <=
    max_loaded_channels.
Proof.
  intros before marker prepared count_safe within_capacity successor_safe.
  destruct successor_safe as [already_loaded|room].
  - rewrite already_loaded_replacement_does_not_change_loaded_count; assumption.
  - destruct (in_dec path_eq_dec marker (loaded_markers before)) as [loaded|not_loaded].
    + rewrite already_loaded_replacement_does_not_change_loaded_count; assumption.
    + rewrite newly_loaded_replacement_increments_loaded_count_once;
        [lia|exact count_safe|exact not_loaded].
Qed.

Theorem target_catalog_is_invalidated_exactly : forall before marker prepared,
  candidate_catalogs (publish_prepared_prefix before marker prepared) marker = None.
Proof.
  intros. unfold publish_prepared_prefix, remove_target_catalog. simpl.
  destruct (path_eq_dec marker marker); congruence.
Qed.

Theorem non_target_catalog_identity_is_preserved : forall
    before marker other prepared,
  other <> marker ->
  candidate_catalogs (publish_prepared_prefix before marker prepared) other =
  candidate_catalogs before other.
Proof.
  intros. unfold publish_prepared_prefix, remove_target_catalog. simpl.
  destruct (path_eq_dec other marker); congruence.
Qed.

Theorem externally_held_catalog_snapshots_remain_valid_immutable_values : forall
    before marker prepared,
  external_catalog_snapshots (publish_prepared_prefix before marker prepared) =
  external_catalog_snapshots before.
Proof. reflexivity. Qed.

Definition catalog_invariant (state : store_state) : Prop :=
  forall marker catalog,
    candidate_catalogs state marker = Some catalog ->
    In marker (loaded_markers state) /\
    catalog_matches catalog
      (prefix_projection marker (authoritative_entries state)).

Theorem non_target_catalog_semantics_are_preserved : forall
    before marker other prepared catalog,
  other <> marker ->
  prefix_independent marker other ->
  catalog_invariant before ->
  candidate_catalogs before other = Some catalog ->
  catalog_matches catalog
    (prefix_projection other
      (authoritative_entries (publish_prepared_prefix before marker prepared))).
Proof.
  intros before marker other prepared catalog different independent valid cached.
  specialize (valid other catalog cached) as [_ matches].
  rewrite published_independent_channel_is_preserved by exact independent.
  exact matches.
Qed.

Theorem publication_preserves_catalog_invariant : forall
    before marker prepared,
  catalog_invariant before ->
  (forall other catalog,
    other <> marker ->
    candidate_catalogs before other = Some catalog ->
    prefix_independent marker other) ->
  catalog_invariant (publish_prepared_prefix before marker prepared).
Proof.
  intros before marker prepared valid independent.
  intros other catalog cached.
  destruct (path_eq_dec other marker) as [same|different].
  - subst. rewrite target_catalog_is_invalidated_exactly in cached. discriminate.
  - assert (candidate_catalogs (publish_prepared_prefix before marker prepared) other =
      candidate_catalogs before other) as catalog_identity.
    { apply non_target_catalog_identity_is_preserved. exact different. }
    rewrite catalog_identity in cached.
    specialize (valid other catalog cached) as [loaded matches]. split.
    + unfold publish_prepared_prefix, mark_loaded. simpl.
      destruct (in_dec path_eq_dec marker (loaded_markers before)); simpl; auto.
    + rewrite published_independent_channel_is_preserved.
      * exact matches.
      * now apply (independent other catalog).
Qed.

Theorem publication_preserves_distinct_count_formula : forall before marker prepared,
  distinct_count
    (authoritative_entries (publish_prepared_prefix before marker prepared)) =
  distinct_count (authoritative_entries before) -
    distinct_count (prefix_projection marker (authoritative_entries before)) +
    distinct_count (prepared_entries prepared).
Proof. intros. apply successful_publication_distinct_count_formula. Qed.

Theorem publication_preserves_cardinality_formula : forall before marker prepared,
  cardinality
    (authoritative_entries (publish_prepared_prefix before marker prepared)) =
  cardinality (authoritative_entries before) -
    cardinality (prefix_projection marker (authoritative_entries before)) +
    cardinality (prepared_entries prepared).
Proof. intros. apply successful_publication_cardinality_formula. Qed.

(** The historical clone-clear-insert-publish transaction is retained only as
    a semantic oracle.  [legacy_insert_prepared] denotes its successful
    clear-and-insert result; it does not authorize a production whole-store
    clone. *)
Definition legacy_insert_prepared
    (cleared : list bag_entry) (marker : path) (prepared : prepared_suffix_bag)
    : list bag_entry :=
  cleared ++ lift_suffix_entries marker (prepared_entries prepared).

Definition legacy_clone_clear_insert_publish
    (before : store_state) (marker : path) (prepared : prepared_suffix_bag)
    : store_state :=
  let loaded := mark_loaded marker (loaded_markers before) in
  StoreState
    (legacy_insert_prepared
      (keep_outside_prefix marker (authoritative_entries before)) marker prepared)
    loaded
    (length loaded)
    (remove_target_catalog marker (candidate_catalogs before))
    (external_catalog_snapshots before).

Theorem prepare_then_publish_equals_legacy_success_semantics : forall
    before marker prepared,
  publish_prepared_prefix before marker prepared =
  legacy_clone_clear_insert_publish before marker prepared.
Proof. reflexivity. Qed.

(** The publication authority is deliberately narrower than a raw PathMap
    zipper: its immutable root is the framed channel marker and every move is
    expressed only as a relative suffix.  There is no constructor for a raw
    root, subtree graft, or absolute out-of-prefix mutation. *)
Record guarded_target_cursor : Type := GuardedTargetCursor {
  cursor_target_root : path;
  cursor_relative_path : path
}.

Definition open_target_cursor (marker : path) : guarded_target_cursor :=
  GuardedTargetCursor marker [].

Definition cursor_rebase_relative
    (cursor : guarded_target_cursor) (suffix : path) : guarded_target_cursor :=
  GuardedTargetCursor (cursor_target_root cursor) suffix.

Definition cursor_absolute_path (cursor : guarded_target_cursor) : path :=
  cursor_target_root cursor ++ cursor_relative_path cursor.

Definition cursor_write_entry
    (cursor : guarded_target_cursor) (entry : bag_entry) : bag_entry :=
  BagEntry
    (cursor_target_root cursor ++ entry_path entry)
    (entry_multiplicity entry).

Definition cursor_replay_entries
    (cursor : guarded_target_cursor) (entries : list bag_entry) : list bag_entry :=
  map (cursor_write_entry cursor) entries.

Theorem relative_cursor_rebase_preserves_immutable_target_root : forall
    cursor suffix,
  cursor_target_root (cursor_rebase_relative cursor suffix) =
  cursor_target_root cursor.
Proof. reflexivity. Qed.

Theorem every_relative_cursor_position_is_inside_its_target_prefix : forall cursor,
  strip_prefix (cursor_target_root cursor) (cursor_absolute_path cursor) =
  Some (cursor_relative_path cursor).
Proof. intros. unfold cursor_absolute_path. apply strip_prefix_composed_path. Qed.

Theorem every_cursor_write_is_inside_its_target_prefix : forall cursor entry,
  strip_prefix (cursor_target_root cursor)
    (entry_path (cursor_write_entry cursor entry)) = Some (entry_path entry).
Proof. intros. simpl. apply strip_prefix_composed_path. Qed.

Theorem prefix_rooted_cursor_replay_is_exact_suffix_lifting : forall
    marker replacement,
  cursor_replay_entries (open_target_cursor marker) replacement =
  lift_suffix_entries marker replacement.
Proof. reflexivity. Qed.

Definition publish_with_prefix_rooted_cursor
    (before : list bag_entry) (cursor : guarded_target_cursor)
    (replacement : list bag_entry) : list bag_entry :=
  keep_outside_prefix (cursor_target_root cursor) before ++
  cursor_replay_entries cursor replacement.

Theorem prefix_rooted_cursor_publication_equals_prefix_replacement : forall
    before marker replacement,
  publish_with_prefix_rooted_cursor before (open_target_cursor marker) replacement =
  replace_prefix before marker replacement.
Proof. reflexivity. Qed.

Theorem prefix_rooted_cursor_cannot_change_non_target_lookup : forall
    before marker replacement selected,
  strip_prefix marker selected = None ->
  lookup_multiplicity selected
    (publish_with_prefix_rooted_cursor before (open_target_cursor marker)
      replacement) =
  lookup_multiplicity selected before.
Proof.
  intros. rewrite prefix_rooted_cursor_publication_equals_prefix_replacement.
  now apply successful_publication_preserves_every_non_target_key.
Qed.

(** The reusable encoder buffer may append a relative suffix, hand the
    composed bytes directly to the one target zipper, then truncate back to
    its immutable marker length. *)
Record reusable_path_buffer : Type := ReusablePathBuffer {
  buffer_marker : path;
  buffer_bytes : path
}.

Definition buffer_at_marker (marker : path) : reusable_path_buffer :=
  ReusablePathBuffer marker marker.

Definition buffer_append_suffix
    (buffer : reusable_path_buffer) (suffix : path) : reusable_path_buffer :=
  ReusablePathBuffer (buffer_marker buffer) (buffer_bytes buffer ++ suffix).

Definition buffer_restore_marker
    (buffer : reusable_path_buffer) : reusable_path_buffer :=
  ReusablePathBuffer (buffer_marker buffer) (buffer_marker buffer).

Theorem reusable_buffer_composes_exact_absolute_key : forall marker suffix,
  buffer_bytes (buffer_append_suffix (buffer_at_marker marker) suffix) =
  marker ++ suffix.
Proof. reflexivity. Qed.

Theorem reusable_buffer_restores_exact_marker_after_every_replay : forall marker suffix,
  buffer_restore_marker (buffer_append_suffix (buffer_at_marker marker) suffix) =
  buffer_at_marker marker.
Proof. reflexivity. Qed.

Fixpoint replay_and_restore_buffer
    (marker : path) (entries : list bag_entry) : reusable_path_buffer :=
  match entries with
  | [] => buffer_at_marker marker
  | entry :: rest =>
      let _written := buffer_bytes
        (buffer_append_suffix (buffer_at_marker marker) (entry_path entry)) in
      replay_and_restore_buffer marker rest
  end.

Theorem reusable_buffer_is_at_marker_after_whole_replay : forall marker entries,
  replay_and_restore_buffer marker entries = buffer_at_marker marker.
Proof.
  intros marker entries. induction entries as [|entry rest IH]; simpl;
    [reflexivity|exact IH].
Qed.

Inductive channel_replacement_invariant_id : Type :=
| StrongFailureAtomicityInvariant
| ExactTargetPrefixInvariant
| NonTargetKeyPreservationInvariant
| IndependentChannelPreservationInvariant
| PrefixSuffixCompositionInvariant
| ReusableBufferRestorationInvariant
| LoadedSetInvariant
| LoadedCountInvariant
| EmptyLoadedChannelInvariant
| TargetCatalogInvalidationInvariant
| NonTargetCatalogIdentityInvariant
| ExternalCatalogSnapshotInvariant
| DistinctCountFormulaInvariant
| CardinalityFormulaInvariant
| MultiplicityPreservationInvariant
| LegacySuccessEquivalenceInvariant
| PreparedBagWellFormedInvariant
| CheckedU64MultiplicityInvariant
| ResultWellFormedInvariant
| PrefixRootedCursorInvariant
| RelativeCursorNoEscapeInvariant
| SingleCursorReplayInvariant
| LoadedCapacityInvariant
| LoadedSetUniquenessInvariant
| PreparationFailurePrecedenceInvariant
| CatalogInvariantPreservationInvariant
| PreparedAggregationRefinementInvariant.

Definition channel_replacement_invariant_catalog :
    list channel_replacement_invariant_id :=
  [StrongFailureAtomicityInvariant; ExactTargetPrefixInvariant;
   NonTargetKeyPreservationInvariant; IndependentChannelPreservationInvariant;
   PrefixSuffixCompositionInvariant; ReusableBufferRestorationInvariant;
   LoadedSetInvariant; LoadedCountInvariant; EmptyLoadedChannelInvariant;
   TargetCatalogInvalidationInvariant; NonTargetCatalogIdentityInvariant;
   ExternalCatalogSnapshotInvariant; DistinctCountFormulaInvariant;
   CardinalityFormulaInvariant; MultiplicityPreservationInvariant;
   LegacySuccessEquivalenceInvariant; PreparedBagWellFormedInvariant;
   CheckedU64MultiplicityInvariant; ResultWellFormedInvariant;
   PrefixRootedCursorInvariant; RelativeCursorNoEscapeInvariant;
   SingleCursorReplayInvariant; LoadedCapacityInvariant;
   LoadedSetUniquenessInvariant; PreparationFailurePrecedenceInvariant;
   CatalogInvariantPreservationInvariant; PreparedAggregationRefinementInvariant].

Theorem channel_replacement_invariant_catalog_is_unique_and_exact :
  NoDup channel_replacement_invariant_catalog /\
  length channel_replacement_invariant_catalog = 27.
Proof. split; [repeat constructor; simpl; intuition discriminate|reflexivity]. Qed.

(** Capstone: all publication effects are derived from one prepared suffix Bag
    and one prefix-rooted cursor.  Fallible preparation is separately covered
    by [every_preparation_failure_preserves_the_entire_public_state]. *)
Record successful_channel_publication_contract
    (before : store_state) (marker : path) (prepared : prepared_suffix_bag)
    : Prop := SuccessfulChannelPublicationContract {
  contract_target_exact :
    prefix_projection marker
      (authoritative_entries (publish_prepared_prefix before marker prepared)) =
    prepared_entries prepared;
  contract_non_target_preserved : forall selected,
    strip_prefix marker selected = None ->
    lookup_multiplicity selected
      (authoritative_entries (publish_prepared_prefix before marker prepared)) =
    lookup_multiplicity selected (authoritative_entries before);
  contract_target_loaded :
    In marker (loaded_markers (publish_prepared_prefix before marker prepared));
  contract_count_matches_set :
    loaded_marker_count (publish_prepared_prefix before marker prepared) =
    length (loaded_markers (publish_prepared_prefix before marker prepared));
  contract_loaded_set_unique :
    NoDup (loaded_markers (publish_prepared_prefix before marker prepared));
  contract_count_within_capacity :
    loaded_marker_count (publish_prepared_prefix before marker prepared) <=
    max_loaded_channels;
  contract_target_catalog_absent :
    candidate_catalogs (publish_prepared_prefix before marker prepared) marker = None;
  contract_other_catalog_identity : forall other,
    other <> marker ->
    candidate_catalogs (publish_prepared_prefix before marker prepared) other =
    candidate_catalogs before other;
  contract_catalog_semantics :
    catalog_invariant (publish_prepared_prefix before marker prepared);
  contract_external_snapshots_identity :
    external_catalog_snapshots (publish_prepared_prefix before marker prepared) =
    external_catalog_snapshots before;
  contract_result_well_formed :
    entries_well_formed
      (authoritative_entries (publish_prepared_prefix before marker prepared));
  contract_result_u64_bounded :
    entries_fit_u64
      (authoritative_entries (publish_prepared_prefix before marker prepared));
  contract_distinct_count :
    distinct_count
      (authoritative_entries (publish_prepared_prefix before marker prepared)) =
    distinct_count (authoritative_entries before) -
      distinct_count (prefix_projection marker (authoritative_entries before)) +
      distinct_count (prepared_entries prepared);
  contract_cardinality :
    cardinality
      (authoritative_entries (publish_prepared_prefix before marker prepared)) =
    cardinality (authoritative_entries before) -
      cardinality (prefix_projection marker (authoritative_entries before)) +
      cardinality (prepared_entries prepared);
  contract_cursor_publication_refines_replacement :
    publish_with_prefix_rooted_cursor
      (authoritative_entries before) (open_target_cursor marker)
      (prepared_entries prepared) =
    authoritative_entries (publish_prepared_prefix before marker prepared);
  contract_legacy_success_equivalent :
    publish_prepared_prefix before marker prepared =
    legacy_clone_clear_insert_publish before marker prepared
}.

Theorem prepare_then_publish_channel_replacement_capstone : forall
    before marker prepared,
  entries_well_formed (authoritative_entries before) ->
  entries_fit_u64 (authoritative_entries before) ->
  loaded_marker_count before = length (loaded_markers before) ->
  NoDup (loaded_markers before) ->
  loaded_marker_count before <= max_loaded_channels ->
  loaded_successor_safe before marker ->
  catalog_invariant before ->
  (forall other catalog,
    other <> marker ->
    candidate_catalogs before other = Some catalog ->
    prefix_independent marker other) ->
  successful_channel_publication_contract before marker prepared.
Proof.
  intros before marker prepared before_well_formed before_bounded count_exact
    loaded_unique within_capacity successor_safe catalog_valid catalog_independent.
  constructor.
  - apply published_target_prefix_is_exact.
  - intros. now apply published_non_target_key_is_preserved.
  - apply target_is_loaded_even_when_replacement_is_empty.
  - apply publication_loaded_count_matches_loaded_set.
  - now apply publication_preserves_loaded_set_uniqueness.
  - eapply publication_respects_preflighted_loaded_capacity; eauto.
  - apply target_catalog_is_invalidated_exactly.
  - intros. now apply non_target_catalog_identity_is_preserved.
  - now apply publication_preserves_catalog_invariant.
  - apply externally_held_catalog_snapshots_remain_valid_immutable_values.
  - now apply publication_preserves_authoritative_well_formedness.
  - now apply publication_preserves_checked_u64_multiplicities.
  - apply publication_preserves_distinct_count_formula.
  - apply publication_preserves_cardinality_formula.
  - apply prefix_rooted_cursor_publication_equals_prefix_replacement.
  - apply prepare_then_publish_equals_legacy_success_semantics.
Qed.

Print Assumptions every_preparation_failure_has_precedence_over_publication.
Print Assumptions successful_publication_exactly_replaces_target_prefix.
Print Assumptions successful_publication_preserves_every_non_target_key.
Print Assumptions replace_prefix_preserves_well_formedness.
Print Assumptions publication_preserves_prepared_input_aggregation.
Print Assumptions publication_respects_preflighted_loaded_capacity.
Print Assumptions prefix_rooted_cursor_cannot_change_non_target_lookup.
Print Assumptions prepare_then_publish_equals_legacy_success_semantics.
Print Assumptions channel_replacement_invariant_catalog_is_unique_and_exact.
Print Assumptions prepare_then_publish_channel_replacement_capstone.

End Transaction.
End ChannelPrefixReplacement.
