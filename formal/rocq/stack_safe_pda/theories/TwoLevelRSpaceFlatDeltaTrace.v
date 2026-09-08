From Stdlib Require Import Bool List Lia NArith ZArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  RhoPathV1Datum
  TwoLevelRSpaceMatchPreparation.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import RhoPathV1Datum.
Import TwoLevelRSpaceMatchPreparation.
Set Implicit Arguments.
Open Scope Z_scope.

Inductive FlatDeltaRecord : Type :=
| RecordAddition (path : Path) (amount : N)
| RecordRemoval (path : Path) (amount : N).

Definition apply_flat_record (delta : FlatBagDelta) (operation : FlatDeltaRecord) :=
  match operation with
  | RecordAddition path amount => flat_record_addition delta path amount
  | RecordRemoval path amount => flat_record_removal delta path amount
  end.

Fixpoint record_flat_trace (delta : FlatBagDelta) (operations : list FlatDeltaRecord)
    : option FlatBagDelta :=
  match operations with
  | [] => Some delta
  | operation :: tail =>
      match apply_flat_record delta operation with
      | None => None
      | Some after => record_flat_trace after tail
      end
  end.

Definition record_logical_change (query : Path) (operation : FlatDeltaRecord) : Z :=
  match operation with
  | RecordAddition path amount => if path_eq_dec query path then Z.of_N amount else 0
  | RecordRemoval path amount => if path_eq_dec query path then - Z.of_N amount else 0
  end.

Fixpoint trace_logical_change (query : Path) (operations : list FlatDeltaRecord) : Z :=
  match operations with
  | [] => 0
  | operation :: tail => record_logical_change query operation + trace_logical_change query tail
  end.

Definition flat_signed_count (query : Path) (delta : FlatBagDelta) : Z :=
  Z.of_N (flat_lookup query (flat_delta_additions delta)) -
  Z.of_N (flat_lookup query (flat_delta_removals delta)).

Lemma flat_record_addition_refines_signed_change : forall delta path amount after query,
  flat_delta_well_formedb delta = true ->
  flat_record_addition delta path amount = Some after ->
  flat_signed_count query after = flat_signed_count query delta +
    record_logical_change query (RecordAddition path amount).
Proof.
  intros delta path amount [adds removes] query Hnormal Hrecord.
  destruct (path_eq_dec query path) as [Hequal|Hdifferent].
  - subst query.
    unfold flat_delta_well_formedb in Hnormal.
    apply andb_true_iff in Hnormal as [Hboth _].
    apply andb_true_iff in Hboth as [Hadd Hremove].
    apply andb_true_iff in Hadd as [_ Hadd].
    apply andb_true_iff in Hremove as [_ Hremove].
    pose proof (@flat_record_addition_is_exact_opposite_cancellation
      delta path amount adds removes Hadd Hremove Hrecord) as [Hcancel Hdest].
    unfold flat_signed_count. cbn [flat_delta_additions flat_delta_removals].
    rewrite Hcancel, Hdest. cbn [record_logical_change].
    destruct (path_eq_dec path path); [|contradiction].
    destruct (N.le_ge_cases (flat_lookup path (flat_delta_removals delta)) amount).
    + rewrite N.min_l by assumption. lia.
    + rewrite N.min_r by assumption. lia.
  - pose proof (@flat_record_addition_preserves_other delta path amount
      (FlatBagDeltaState adds removes) query (not_eq_sym Hdifferent) Hrecord) as [Hadd Hremove].
    unfold flat_signed_count. rewrite Hadd, Hremove. cbn [record_logical_change].
    destruct (path_eq_dec query path); [contradiction|lia].
Qed.

Lemma flat_record_removal_refines_signed_change : forall delta path amount after query,
  flat_delta_well_formedb delta = true ->
  flat_record_removal delta path amount = Some after ->
  flat_signed_count query after = flat_signed_count query delta +
    record_logical_change query (RecordRemoval path amount).
Proof.
  intros delta path amount [adds removes] query Hnormal Hrecord.
  destruct (path_eq_dec query path) as [Hequal|Hdifferent].
  - subst query.
    unfold flat_delta_well_formedb in Hnormal.
    apply andb_true_iff in Hnormal as [Hboth _].
    apply andb_true_iff in Hboth as [Hadd Hremove].
    apply andb_true_iff in Hadd as [_ Hadd].
    apply andb_true_iff in Hremove as [_ Hremove].
    pose proof (@flat_record_removal_is_exact_opposite_cancellation
      delta path amount adds removes Hadd Hremove Hrecord) as [Hcancel Hdest].
    unfold flat_signed_count. cbn [flat_delta_additions flat_delta_removals].
    rewrite Hcancel, Hdest. cbn [record_logical_change].
    destruct (path_eq_dec path path); [|contradiction].
    destruct (N.le_ge_cases (flat_lookup path (flat_delta_additions delta)) amount).
    + rewrite N.min_l by assumption. lia.
    + rewrite N.min_r by assumption. lia.
  - pose proof (@flat_record_removal_preserves_other delta path amount
      (FlatBagDeltaState adds removes) query (not_eq_sym Hdifferent) Hrecord) as [Hadd Hremove].
    unfold flat_signed_count. rewrite Hadd, Hremove. cbn [record_logical_change].
    destruct (path_eq_dec query path); [contradiction|lia].
Qed.

Lemma apply_flat_record_preserves_normalization : forall delta operation after,
  flat_delta_well_formedb delta = true ->
  apply_flat_record delta operation = Some after ->
  flat_delta_well_formedb after = true.
Proof.
  intros delta [path amount|path amount] after Hnormal Hrecord.
  - exact (@flat_record_addition_preserves_normalization delta path amount after Hnormal Hrecord).
  - exact (@flat_record_removal_preserves_normalization delta path amount after Hnormal Hrecord).
Qed.

Lemma apply_flat_record_refines_signed_change : forall delta operation after query,
  flat_delta_well_formedb delta = true ->
  apply_flat_record delta operation = Some after ->
  flat_signed_count query after = flat_signed_count query delta + record_logical_change query operation.
Proof.
  intros delta [path amount|path amount] after query Hnormal Hrecord.
  - exact (@flat_record_addition_refines_signed_change delta path amount after query Hnormal Hrecord).
  - exact (@flat_record_removal_refines_signed_change delta path amount after query Hnormal Hrecord).
Qed.

Theorem finite_flat_record_trace_refines_logical_sum : forall operations delta after,
  flat_delta_well_formedb delta = true ->
  record_flat_trace delta operations = Some after ->
  flat_delta_well_formedb after = true /\
  forall query, flat_signed_count query after =
    flat_signed_count query delta + trace_logical_change query operations.
Proof.
  induction operations as [|operation tail IH]; intros delta after Hnormal Htrace.
  - cbn [record_flat_trace] in Htrace. inversion Htrace; subst after.
    split; [exact Hnormal|intro query; cbn [trace_logical_change]; lia].
  - cbn [record_flat_trace] in Htrace.
    destruct (apply_flat_record delta operation) as [next|] eqn:Hstep; [|discriminate].
    pose proof (@apply_flat_record_preserves_normalization delta operation next Hnormal Hstep) as Hnext.
    destruct (IH next after Hnext Htrace) as [Hafter Hsum]. split; [exact Hafter|].
    intro query. rewrite Hsum.
    rewrite (@apply_flat_record_refines_signed_change delta operation next query Hnormal Hstep).
    cbn [trace_logical_change]. lia.
Qed.

Theorem normalized_supports_are_separate_signed_projections : forall delta query,
  flat_delta_well_formedb delta = true ->
  Z.of_N (flat_lookup query (flat_delta_additions delta)) = Z.max (flat_signed_count query delta) 0 /\
  Z.of_N (flat_lookup query (flat_delta_removals delta)) = Z.max (- flat_signed_count query delta) 0.
Proof.
  intros [adds removes] query Hnormal.
  unfold flat_delta_well_formedb in Hnormal.
  apply andb_true_iff in Hnormal as [Hboth Hdisjoint].
  apply andb_true_iff in Hboth as [Hadd Hremove].
  pose proof ((proj1 (@flat_disjoint_is_pointwise adds removes Hadd)) Hdisjoint query) as Hzero.
  unfold flat_signed_count. cbn [flat_delta_additions flat_delta_removals].
  destruct Hzero as [Hzero|Hzero]; rewrite Hzero; simpl.
  - split; [rewrite Z.max_r by lia|rewrite Z.max_l by lia]; lia.
  - split; [rewrite Z.max_l by lia|rewrite Z.max_r by lia]; lia.
Qed.

Theorem empty_flat_record_trace_has_exact_separate_supports : forall operations after,
  record_flat_trace empty_flat_delta operations = Some after ->
  forall query,
  Z.of_N (flat_lookup query (flat_delta_additions after)) = Z.max (trace_logical_change query operations) 0 /\
  Z.of_N (flat_lookup query (flat_delta_removals after)) = Z.max (- trace_logical_change query operations) 0.
Proof.
  intros operations after Htrace query.
  destruct (@finite_flat_record_trace_refines_logical_sum operations empty_flat_delta after eq_refl Htrace)
    as [Hnormal Hsum].
  pose proof (@normalized_supports_are_separate_signed_projections after query Hnormal) as Hsides.
  rewrite Hsum in Hsides. cbn [flat_signed_count empty_flat_delta flat_delta_additions
    flat_delta_removals flat_lookup lookup_multiplicity_in_bag] in Hsides.
  exact Hsides.
Qed.

Theorem finite_flat_trace_partitions_into_exact_logical_supports : forall operations delta changes,
  record_flat_trace empty_flat_delta operations = Some delta ->
  parse_all_flat_changes delta = BuildOk changes ->
  exists groups,
    partition_flat_delta delta = BuildOk groups /\
    partition_groups_valid groups /\
    forall absolute view,
      parse_absolute_datum absolute = Some view ->
      Z.of_N (grouped_count ChangeAddition (decoded_view_marker view)
        (decoded_view_relative view) groups) = Z.max (trace_logical_change absolute operations) 0 /\
      Z.of_N (grouped_count ChangeRemoval (decoded_view_marker view)
        (decoded_view_relative view) groups) = Z.max (- trace_logical_change absolute operations) 0.
Proof.
  intros operations delta changes Htrace Hparse.
  destruct (@finite_flat_record_trace_refines_logical_sum operations empty_flat_delta delta eq_refl Htrace)
    as [Hnormal Hsum].
  destruct (@normalized_parsed_partition_exists delta changes Hnormal Hparse)
    as [groups [Hpartition [Hgroups Hrest]]].
  exists groups. split; [exact Hpartition|]. split; [exact Hgroups|].
  intros absolute view Hview.
  destruct (@grouped_partition_preserves_both_flat_sides delta groups Hnormal Hpartition)
    as [_ Hcounts].
  destruct (Hcounts absolute view Hview) as [Hadd Hremove].
  rewrite Hadd, Hremove.
  exact (@empty_flat_record_trace_has_exact_separate_supports operations delta Htrace absolute).
Qed.

Print Assumptions finite_flat_record_trace_refines_logical_sum.
Print Assumptions normalized_supports_are_separate_signed_projections.
Print Assumptions empty_flat_record_trace_has_exact_separate_supports.
Print Assumptions finite_flat_trace_partitions_into_exact_logical_supports.
