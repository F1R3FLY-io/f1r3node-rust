From Stdlib Require Import List Arith Bool Lia.
From StackSafePDA Require Import EPM1 SubstitutionRegionFraming
  SubstitutionRegionOrdinals SubstitutionRegionProvenance SubstitutionRegionRanges.

Import ListNotations.
Set Implicit Arguments.
Module SubstitutionValueRegions.
Module F := SubstitutionRegionFraming.SubstitutionRegionFraming.
Module O := SubstitutionRegionOrdinals.SubstitutionRegionOrdinals.
Module V := SubstitutionRegionProvenance.SubstitutionRegionProvenance.
Module R := SubstitutionRegionRanges.SubstitutionRegionRanges.

Record span := Span { start : nat; finish : nat }.
Definition body input r := F.interval input (start r) (finish r).

Definition scan_frame (input : bytes) (position : nat) : option span :=
  match F.scan_groups 10 true (skipn position input) with
  | Some (size, after_length) =>
      if size <=? length after_length then
        let lo := position + (length (skipn position input) - length after_length) in
        Some (Span lo (lo + size))
      else None
  | None => None
  end.

Fixpoint scan_values count input position : option (list span * nat) :=
  match count with
  | 0 => Some ([], position)
  | S count =>
      match scan_frame input position with
      | Some r =>
          match scan_values count input (finish r) with
          | Some (rs, ending) => Some (r :: rs, ending)
          | None => None
          end
      | None => None
      end
  end.

Definition inspect_values count input position : option (list span) :=
  if (position <=? length input) && (count <=? length input - position) then
    match scan_values count input position with
    | Some (rs, ending) => if ending =? length input then Some rs else None
    | None => None
    end
  else None.

Lemma suffix_split_gives_exact_offset : forall (input : bytes) position (header suffix : bytes),
  position <= length input ->
  skipn position input = header ++ suffix ->
  position + length header <= length input /\
  skipn (position + length header) input = suffix /\
  position + (length (skipn position input) - length suffix) = position + length header.
Proof.
  intros input position header suffix Bound Split.
  pose proof (f_equal (@length nat) Split) as Length.
  rewrite length_skipn, length_app in Length.
  split; [lia|split; [|rewrite length_skipn; lia]].
  rewrite Nat.add_comm, <- skipn_skipn, Split, skipn_app, skipn_all, Nat.sub_diag; reflexivity.
Qed.

Theorem single_frame_selects_exact_physical_range : forall input position r,
  position <= length input -> scan_frame input position = Some r ->
  position < start r /\ start r <= finish r /\ finish r <= length input /\
  start r = position + length (encode_varint (length (body input r))) /\
  decode_frame (skipn position input) =
    Some (body input r, skipn (finish r) input).
Proof.
  intros input position r Bound Scan; unfold scan_frame in Scan.
  destruct (F.scan_groups 10 true (skipn position input)) as [[size rest]|] eqn:Header;
    try discriminate.
  destruct (size <=? length rest) eqn:Enough; try discriminate.
  apply Nat.leb_le in Enough; inversion Scan; subst r; clear Scan.
  pose proof (@F.successful_bounded_varint_reencodes_consumed_prefix
    (skipn position input) size rest Header) as Split.
  destruct (@suffix_split_gives_exact_offset input position (encode_varint size) rest Bound Split)
    as [StartBound [AtStart Exact]].
  pose proof (f_equal (@length nat) Split) as Length.
  rewrite length_skipn, length_app in Length.
  pose proof (O.encoded_varint_is_nonempty size) as Positive.
  cbn [start finish]; rewrite Exact.
  assert (Body : body input (Span (position + length (encode_varint size))
      (position + length (encode_varint size) + size)) = firstn size rest).
  { unfold body, F.interval; cbn [start finish].
    replace (position + length (encode_varint size) + size -
      (position + length (encode_varint size))) with size by lia.
    now rewrite AtStart. }
  assert (BodyLength : length (firstn size rest) = size).
  { rewrite length_firstn; apply Nat.min_l; exact Enough. }
  repeat split; try lia.
  - rewrite Body, BodyLength; reflexivity.
  - unfold decode_frame.
    rewrite (proj1 (@F.successful_groups_refine_decoder 10 true
      (skipn position input) size rest Header)).
    assert (EnoughBool : (size <=? length rest) = true) by (apply Nat.leb_le; exact Enough).
    rewrite EnoughBool, Body.
    replace (position + length (encode_varint size) + size) with
      (size + (position + length (encode_varint size))) by lia.
    rewrite <- skipn_skipn, AtStart; reflexivity.
Qed.

Inductive partition input : nat -> list span -> nat -> Prop :=
| PartitionEmpty : forall position, partition input position [] position
| PartitionNext : forall position r rs ending,
    position < start r -> start r <= finish r -> finish r <= length input ->
    start r = position + length (encode_varint (length (body input r))) ->
    partition input (finish r) rs ending ->
    partition input position (r :: rs) ending.

Theorem successful_loop_refines_decoder_and_partition : forall count input position rs ending,
  position <= length input -> scan_values count input position = Some (rs, ending) ->
  partition input position rs ending /\ length rs = count /\ ending <= length input /\
  decode_frames count (skipn position input) =
    Some (map (body input) rs, skipn ending input).
Proof.
  induction count as [|count IH]; intros input position rs ending Bound Scan.
  - cbn [scan_values] in Scan; inversion Scan; subst.
    repeat split; auto; constructor.
  - cbn [scan_values] in Scan.
    destruct (scan_frame input position) as [r|] eqn:Frame; try discriminate.
    destruct (scan_values count input (finish r)) as [[rest stop]|] eqn:Tail; try discriminate.
    inversion Scan; subst rs ending; clear Scan.
    destruct (@single_frame_selects_exact_physical_range input position r Bound Frame)
      as [Progress [Order [Limit [Exact Decoded]]]].
    destruct (IH input (finish r) rest stop Limit Tail) as [Part [Count [Stop DecodeTail]]].
    split; [econstructor; eauto|split; [cbn; lia|split; auto]].
    cbn [decode_frames map]; now rewrite Decoded, DecodeTail.
Qed.

Theorem partition_has_exact_count_bound : forall input position rs ending,
  partition input position rs ending ->
  position <= ending /\ length rs <= ending - position.
Proof.
  intros input position rs ending Part; induction Part; cbn; intuition lia.
Qed.

Theorem partition_retains_ordered_in_bounds_spans : forall input position rs ending,
  partition input position rs ending ->
  Forall (fun r => position < start r /\ start r <= finish r /\ finish r <= ending) rs.
Proof.
  intros input position rs ending Part; induction Part.
  - constructor.
  - pose proof (partition_has_exact_count_bound Part) as Bounds.
    constructor; [intuition lia|].
    eapply Forall_impl; [|exact IHPart]; intros r0 P; intuition lia.
Qed.

Fixpoint framed_length input rs :=
  match rs with
  | [] => 0
  | r :: rest => length (encode_frame (body input r)) + framed_length input rest
  end.

Lemma span_body_length : forall input r,
  start r <= finish r -> finish r <= length input ->
  length (body input r) = finish r - start r.
Proof.
  intros input r Order Limit; unfold body, F.interval.
  rewrite length_firstn, length_skipn; apply Nat.min_l; lia.
Qed.

Theorem partition_has_exact_physical_positions : forall input position rs ending,
  partition input position rs ending ->
  forall i r, nth_error rs i = Some r ->
  start r = position + framed_length input (firstn i rs) +
      length (encode_varint (length (body input r))) /\
  finish r = start r + length (body input r).
Proof.
  intros input position rs ending Part; induction Part; intros i target N.
  - destruct i; discriminate.
  - destruct i as [|i].
    + cbn in N; inversion N; subst; cbn.
      rewrite Nat.add_0_r; split; auto.
      rewrite span_body_length by assumption; lia.
    + cbn in N; destruct (IHPart i target N) as [Start End].
      cbn [firstn framed_length]; split; [|exact End].
      unfold encode_frame; rewrite length_app.
      pose proof (span_body_length input r H0 H1) as BodyLength; lia.
Qed.

Theorem successful_inspection_decodes_complete_suffix : forall count input position rs,
  inspect_values count input position = Some rs ->
  partition input position rs (length input) /\ length rs = count /\
  decode_frames count (skipn position input) = Some (map (body input) rs, []).
Proof.
  intros count input position rs H; unfold inspect_values in H.
  destruct (_ && _) eqn:Bounds; try discriminate.
  apply andb_true_iff in Bounds; destruct Bounds as [Start Count].
  apply Nat.leb_le in Start; apply Nat.leb_le in Count.
  destruct (scan_values count input position) as [[regions ending]|] eqn:Scan; try discriminate.
  destruct (ending =? length input) eqn:End; try discriminate.
  apply Nat.eqb_eq in End; inversion H; subst.
  destruct (@successful_loop_refines_decoder_and_partition count input position rs
    (length input) Start Scan) as [Part [Arity [_ Decode]]].
  rewrite skipn_all in Decode; auto.
Qed.

Theorem successful_selection_is_machine_representable : forall count input position rs ending limit,
  length input <= limit -> position <= length input ->
  scan_values count input position = Some (rs, ending) ->
  Forall (fun r => start r <= limit /\ finish r <= limit /\
    length (body input r) <= limit) rs.
Proof.
  intros count input position rs ending limit Limit Bound Scan.
  destruct (@successful_loop_refines_decoder_and_partition count input position rs ending Bound Scan)
    as [Part [_ [End _]]].
  pose proof (partition_retains_ordered_in_bounds_spans Part) as Ranges.
  eapply Forall_impl; [|exact Ranges]; intros r [Start [Order Stop]].
  rewrite span_body_length by lia; intuition lia.
Qed.

Theorem trailing_bytes_refuse_publication : forall count input position rs ending,
  position <= length input -> count <= length input - position ->
  scan_values count input position = Some (rs, ending) ->
  ending <> length input -> inspect_values count input position = None.
Proof.
  intros; unfold inspect_values.
  rewrite (proj2 (Nat.leb_le _ _) H), (proj2 (Nat.leb_le _ _) H0); cbn.
  rewrite H1, (proj2 (Nat.eqb_neq _ _) H2); reflexivity.
Qed.

Theorem impossible_count_refuses_before_loop : forall count input position,
  length input - position < count -> inspect_values count input position = None.
Proof.
  intros; unfold inspect_values.
  rewrite (proj2 (Nat.leb_gt _ _) H), andb_false_r; reflexivity.
Qed.

Theorem failed_frame_cannot_publish_range_list : forall count input position,
  scan_frame input position = None ->
  scan_values (S count) input position = None.
Proof. intros; cbn [scan_values]; now rewrite H. Qed.

Example duplicate_empty_bodies_have_distinct_positions :
  inspect_values 3 [0; 1; 42; 0] 0 =
    Some [Span 1 1; Span 2 3; Span 4 4].
Proof. reflexivity. Qed.

Example truncated_payload_does_not_publish :
  inspect_values 1 [2; 42] 0 = None.
Proof. reflexivity. Qed.

Theorem inspected_prefix_is_in_bounds : forall count input position rs,
  inspect_values count input position = Some rs -> position <= length input.
Proof.
  intros count input position rs H; unfold inspect_values in H.
  destruct (_ && _) eqn:Bounds; try discriminate.
  apply andb_true_iff in Bounds; apply Nat.leb_le; exact (proj1 Bounds).
Qed.

Theorem completed_loop_satisfies_admission_guard : forall count input position rs,
  position <= length input ->
  scan_values count input position = Some (rs, length input) ->
  inspect_values count input position = Some rs.
Proof.
  intros count input position rs Bound Scan.
  destruct (@successful_loop_refines_decoder_and_partition count input position rs
    (length input) Bound Scan) as [Part [Count _]].
  pose proof (partition_has_exact_count_bound Part) as Consumed.
  assert (Guard : (count <=? length input - position) = true) by
    (apply Nat.leb_le; lia).
  unfold inspect_values; rewrite (proj2 (Nat.leb_le _ _) Bound), Guard; cbn.
  now rewrite Scan, Nat.eqb_refl.
Qed.

Theorem region_interpretation_has_exact_length : forall backing payload,
  V.region_valid backing payload ->
  length (V.region_bytes backing payload) = R.last payload - R.first payload.
Proof.
  intros backing payload Valid.
  unfold V.region_bytes, F.interval.
  rewrite length_firstn, length_skipn.
  unfold V.region_valid, R.valid in Valid.
  apply Nat.min_l; apply Nat.sub_le_mono_r; exact (proj2 Valid).
Qed.

Definition lift_regions payload rs :=
  map (fun r => R.child payload (start r) (finish r)) rs.

Theorem successful_selection_lifts_owned_regions : forall backing payload count position rs,
  V.region_valid backing payload ->
  inspect_values count (V.region_bytes backing payload) position = Some rs ->
  Forall (fun r => V.contained r payload /\ R.owner r = R.owner payload)
    (lift_regions payload rs) /\
  map (V.region_bytes backing) (lift_regions payload rs) =
    map (body (V.region_bytes backing payload)) rs.
Proof.
  intros backing payload count position rs Valid Scan.
  destruct (@successful_inspection_decodes_complete_suffix count
    (V.region_bytes backing payload) position rs Scan) as [Part _].
  pose proof (partition_retains_ordered_in_bounds_spans Part) as Ranges.
  pose proof (region_interpretation_has_exact_length Valid) as Size.
  split.
  - unfold lift_regions; apply Forall_map.
    eapply Forall_impl; [|exact Ranges]; intros r [Start [Order Stop]].
    unfold V.contained, R.child; cbn; split; [|reflexivity].
    unfold V.region_valid, R.valid in Valid; intuition lia.
  - unfold lift_regions; rewrite map_map; apply map_ext_in; intros r InRange.
    apply V.subregion_bytes_are_parent_interval; auto.
    apply Forall_forall with (x := r) in Ranges; auto.
    unfold R.local_valid; intuition lia.
Qed.

Theorem selected_physical_ranges_establish_region_field_certificate :
  forall backing t a parent step payload prefix_end count rs tag,
  V.region_valid backing payload ->
  inspect_values count (V.region_bytes backing payload) prefix_end = Some rs ->
  let after := V.erase backing
    (V.add_epathmap t parent step payload (R.child payload 0 prefix_end)
      (lift_regions payload rs)) in
  V.P.field_certificate (V.T.cut_bytes after) (V.T.prefixes after) (V.T.indices after) a
    [(V.E.EPathMap tag (length (V.groups t)),
      V.E.delimited tag (V.region_bytes backing payload))].
Proof.
  intros backing t a parent step payload prefix_end count rs tag Valid Scan; cbn zeta.
  apply V.decoded_epm_suffix_establishes_region_field_certificate with (count := count); auto.
  - pose proof (@inspected_prefix_is_in_bounds count
      (V.region_bytes backing payload) prefix_end rs Scan) as Bound.
    rewrite (region_interpretation_has_exact_length Valid) in Bound.
    unfold R.local_valid; lia.
  - destruct (@successful_selection_lifts_owned_regions backing payload count prefix_end rs
      Valid Scan) as [_ Bytes].
    rewrite Bytes.
    exact (proj2 (proj2 (@successful_inspection_decodes_complete_suffix count
      (V.region_bytes backing payload) prefix_end rs Scan))).
Qed.

Theorem selected_physical_ranges_preserve_all_retained_provenance :
  forall backing root t a parent step payload prefix_end count rs,
  V.region_valid backing root -> V.path_chain root (V.paths t) ->
  V.cuts_linked t -> V.groups_linked t ->
  V.admitted root (V.paths t) parent payload ->
  V.T.table_arena_valid (V.erase backing t) a ->
  inspect_values count (V.region_bytes backing payload) prefix_end = Some rs ->
  let after := V.add_epathmap t parent step payload (R.child payload 0 prefix_end)
      (lift_regions payload rs) in
  V.path_chain root (V.paths after) /\ V.cuts_linked after /\ V.groups_linked after /\
  Forall (fun g => V.region_valid backing (V.whole g) /\ V.region_valid backing (V.prefix g))
    (V.groups after) /\
  V.T.table_arena_valid (V.erase backing after) a.
Proof.
  intros backing root t a parent step payload prefix_end count rs
    Root Chain Cuts Groups Context Arena Scan; cbn zeta.
  assert (Valid : V.region_valid backing payload).
  { eapply V.admitted_region_is_valid; eauto. }
  apply V.region_registration_preserves_all_retained_provenance; auto.
  - pose proof (@inspected_prefix_is_in_bounds count
      (V.region_bytes backing payload) prefix_end rs Scan) as Bound.
    rewrite (region_interpretation_has_exact_length Valid) in Bound.
    unfold V.contained, R.child; cbn.
    unfold V.region_valid, R.valid in Valid; intuition lia.
  - destruct (@successful_selection_lifts_owned_regions backing payload count prefix_end rs
      Valid Scan) as [Regions _].
    eapply Forall_impl; [|exact Regions]; intros r H; exact (proj1 H).
Qed.

Theorem unrepresentable_payload_cannot_be_selected : forall input position size rest limit,
  length input <= limit ->
  F.scan_groups 10 true (skipn position input) = Some (size, rest) ->
  limit < size -> scan_frame input position = None.
Proof.
  intros input position size rest limit Limit Header TooLarge.
  pose proof (@F.successful_bounded_varint_reencodes_consumed_prefix
    (skipn position input) size rest Header) as Split.
  pose proof (f_equal (@length nat) Split) as Length.
  rewrite length_skipn, length_app in Length.
  assert (Enough : (size <=? length rest) = false) by (apply Nat.leb_gt; lia).
  unfold scan_frame; now rewrite Header, Enough.
Qed.

Theorem selected_headers_and_bodies_reconstruct_whole_input : forall count input position rs,
  inspect_values count input position = Some rs ->
  input = firstn position input ++ encode_frames (map (body input) rs).
Proof.
  intros count input position rs Scan.
  destruct (@successful_inspection_decodes_complete_suffix count input position rs Scan)
    as [_ [_ Decode]].
  pose proof (@F.successful_frames_reencode_consumed_prefix count
    (skipn position input) (map (body input) rs) [] Decode) as Suffix.
  rewrite app_nil_r in Suffix; rewrite <- Suffix; symmetry; apply firstn_skipn.
Qed.

Print Assumptions suffix_split_gives_exact_offset.
Print Assumptions single_frame_selects_exact_physical_range.
Print Assumptions successful_loop_refines_decoder_and_partition.
Print Assumptions partition_has_exact_count_bound.
Print Assumptions partition_retains_ordered_in_bounds_spans.
Print Assumptions span_body_length.
Print Assumptions partition_has_exact_physical_positions.
Print Assumptions successful_inspection_decodes_complete_suffix.
Print Assumptions successful_selection_is_machine_representable.
Print Assumptions trailing_bytes_refuse_publication.
Print Assumptions impossible_count_refuses_before_loop.
Print Assumptions failed_frame_cannot_publish_range_list.
Print Assumptions duplicate_empty_bodies_have_distinct_positions.
Print Assumptions truncated_payload_does_not_publish.
Print Assumptions inspected_prefix_is_in_bounds.
Print Assumptions completed_loop_satisfies_admission_guard.
Print Assumptions region_interpretation_has_exact_length.
Print Assumptions successful_selection_lifts_owned_regions.
Print Assumptions selected_physical_ranges_establish_region_field_certificate.
Print Assumptions selected_physical_ranges_preserve_all_retained_provenance.
Print Assumptions unrepresentable_payload_cannot_be_selected.
Print Assumptions selected_headers_and_bodies_reconstruct_whole_input.
End SubstitutionValueRegions.
