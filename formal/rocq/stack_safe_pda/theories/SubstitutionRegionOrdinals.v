From Stdlib Require Import List Arith Bool Lia.
From StackSafePDA Require Import EPM1 SubstitutionRegionFraming SubstitutionRegionDecomposition.

Import ListNotations.
Import SubstitutionRegionFraming.SubstitutionRegionFraming.
Import SubstitutionRegionDecomposition.SubstitutionRegionDecomposition.
Set Implicit Arguments.

Module SubstitutionRegionOrdinals.

Record state := State { last_tag : nat; ordinal : nat }.
Definition initial := State 0 0.
Definition advance (s : state) (tag : nat) : state :=
  State tag (if last_tag s =? tag then S (ordinal s) else 0).
Definition counted (seen : nat) (s : state) : Prop :=
  match seen with 0 => s = initial | S _ => ordinal s < seen end.
Definition checked_advance (limit : nat) (s : state) (tag : nat) : option state :=
  let next := advance s tag in
  if ordinal next <=? limit then Some next else None.

Theorem initial_state_is_counted : counted 0 initial.
Proof. reflexivity. Qed.

Theorem next_ordinal_is_bounded_by_seen_fields : forall seen s tag,
  counted seen s -> 0 < tag -> ordinal (advance s tag) <= seen.
Proof.
  intros [|seen] s tag C T.
  - cbn [counted] in C; subst s.
    unfold advance, initial; cbn.
    destruct tag; cbn; lia.
  - cbn [counted] in C; unfold advance; cbn.
    destruct (last_tag s =? tag); lia.
Qed.

Theorem advance_preserves_counted_state : forall seen s tag,
  counted seen s -> 0 < tag -> counted (S seen) (advance s tag).
Proof.
  intros; cbn [counted].
  pose proof (@next_ordinal_is_bounded_by_seen_fields seen s tag H H0); lia.
Qed.

Theorem input_bounded_step_cannot_overflow : forall limit seen s tag,
  counted seen s -> 0 < tag -> seen < limit ->
  checked_advance limit s tag = Some (advance s tag).
Proof.
  intros limit seen s tag C T L; unfold checked_advance.
  pose proof (@next_ordinal_is_bounded_by_seen_fields seen s tag C T).
  assert (Fits : (ordinal (advance s tag) <=? limit) = true) by
    (apply Nat.leb_le; lia).
  now rewrite Fits.
Qed.

Theorem tag_change_resets_ordinal : forall s tag,
  last_tag s <> tag -> ordinal (advance s tag) = 0.
Proof.
  intros; unfold advance; cbn.
  destruct (last_tag s =? tag) eqn:E; auto.
  apply Nat.eqb_eq in E; contradiction.
Qed.

Theorem positive_tag_run_preserves_count : forall tags seen s,
  counted seen s -> Forall (fun tag => 0 < tag) tags ->
  counted (seen + length tags) (fold_left advance tags s).
Proof.
  induction tags as [|tag rest IH]; intros seen s C T.
  - cbn; now rewrite Nat.add_0_r.
  - inversion T; subst.
    cbn [fold_left length].
    replace (seen + S (length rest)) with (S seen + length rest) by lia.
    apply IH; auto using advance_preserves_counted_state.
Qed.

Lemma encoded_varint_is_nonempty : forall value,
  0 < length (encode_varint value).
Proof.
  intro value; unfold encode_varint; cbn [encode_varint_fuel].
  destruct (value <? 128); cbn; lia.
Qed.

Theorem successful_field_has_positive_tag_and_extent : forall max_tag input f rest,
  scan_field max_tag input = Some (f, rest) ->
  0 < field_tag f /\ 0 < length (field_bytes f).
Proof.
  intros max_tag input f rest H; unfold scan_field in H.
  destruct (scan_groups 10 true input) as [[key after_key]|]; try discriminate.
  destruct ((0 <? key / 8) && (key / 8 <=? max_tag)) eqn:Tag; try discriminate.
  destruct (wire_of key) as [kind|]; try discriminate.
  destruct (scan_body kind after_key) as [[[body encoded] tail]|]; try discriminate.
  inversion H; subst; clear H.
  apply andb_true_iff in Tag; destruct Tag as [Tag _].
  apply Nat.ltb_lt in Tag.
  cbn [field_tag field_key field_bytes].
  split; [exact Tag|rewrite length_app; pose proof (encoded_varint_is_nonempty key); lia].
Qed.

Inductive field_trace (max_tag : nat) : bytes -> list nat -> bytes -> Prop :=
| TraceEmpty : forall rest, field_trace max_tag rest [] rest
| TraceNext : forall input f rest tags suffix,
    scan_field max_tag input = Some (f, rest) ->
    field_trace max_tag rest tags suffix ->
    field_trace max_tag input (field_tag f :: tags) suffix.

Theorem successful_field_trace_bounds_count_and_tags : forall max_tag input tags rest,
  field_trace max_tag input tags rest ->
  length tags + length rest <= length input /\ Forall (fun tag => 0 < tag) tags.
Proof.
  intros max_tag input tags rest H; induction H.
  - split; [cbn; lia|constructor].
  - destruct IHfield_trace as [Bound Tags].
    pose proof (@successful_field_has_positive_tag_and_extent max_tag input f rest H) as [Tag Extent].
    pose proof (@successful_field_scan_reconstructs_source max_tag input f rest H) as Source.
    split.
    + rewrite Source, length_app; cbn [length].
      apply Nat.le_trans with (m := S (length rest)).
      * change (S (length tags + length suffix) <= S (length rest)).
        apply le_n_S; exact Bound.
      * change (1 + length rest <= length (field_bytes f) + length rest).
        apply Nat.add_le_mono_r; exact Extent.
    + constructor; assumption.
Qed.

Theorem successful_prefix_makes_next_ordinal_machine_representable :
  forall max_tag input tags rest f suffix limit,
  field_trace max_tag input tags rest ->
  scan_field max_tag rest = Some (f, suffix) ->
  length input <= limit ->
  checked_advance limit (fold_left advance tags initial) (field_tag f) =
  Some (advance (fold_left advance tags initial) (field_tag f)).
Proof.
  intros max_tag input tags rest f suffix limit Trace Next Bound.
  destruct (@successful_field_trace_bounds_count_and_tags max_tag input tags rest Trace) as [Count Tags].
  destruct (@successful_field_has_positive_tag_and_extent max_tag rest f suffix Next) as [Tag Extent].
  pose proof (@successful_field_scan_reconstructs_source max_tag rest f suffix Next) as Source.
  rewrite Source, length_app in Count.
  apply input_bounded_step_cannot_overflow with (seen := length tags); auto; [|lia].
  replace (length tags) with (0 + length tags) by lia.
  apply positive_tag_run_preserves_count; auto using initial_state_is_counted.
Qed.

Theorem epm_framed_value_count_is_bounded_by_bytes : forall values,
  length values <= length (encode_frames values).
Proof.
  induction values as [|value rest IH]; [reflexivity|].
  change (S (length rest) <= length ((encode_varint (length value) ++ value) ++ encode_frames rest)).
  rewrite !length_app.
  apply Nat.le_trans with (m := S (length (encode_frames rest))).
  - apply le_n_S; exact IH.
  - change (1 + length (encode_frames rest) <=
      (length (encode_varint (length value)) + length value) + length (encode_frames rest)).
    apply Nat.add_le_mono_r.
    eapply Nat.le_trans; [apply encoded_varint_is_nonempty|apply Nat.le_add_r].
Qed.

Theorem epm_enumeration_ordinal_is_machine_representable :
  forall prefix values limit index,
  length (prefix ++ encode_frames values) <= limit ->
  index < length values -> index < limit.
Proof.
  intros prefix values limit index Bound Index.
  pose proof (epm_framed_value_count_is_bounded_by_bytes values).
  rewrite length_app in Bound; lia.
Qed.

Theorem narrow_counter_is_not_justified_by_wider_input : forall narrow wide,
  narrow < wide -> exists ordinal, ordinal <= wide /\ narrow < ordinal.
Proof. intros; exists (S narrow); lia. Qed.

Print Assumptions initial_state_is_counted.
Print Assumptions next_ordinal_is_bounded_by_seen_fields.
Print Assumptions advance_preserves_counted_state.
Print Assumptions input_bounded_step_cannot_overflow.
Print Assumptions tag_change_resets_ordinal.
Print Assumptions positive_tag_run_preserves_count.
Print Assumptions encoded_varint_is_nonempty.
Print Assumptions successful_field_has_positive_tag_and_extent.
Print Assumptions successful_field_trace_bounds_count_and_tags.
Print Assumptions successful_prefix_makes_next_ordinal_machine_representable.
Print Assumptions epm_framed_value_count_is_bounded_by_bytes.
Print Assumptions epm_enumeration_ordinal_is_machine_representable.
Print Assumptions narrow_counter_is_not_justified_by_wider_input.

Theorem widening_preserves_successful_steps : forall narrow wide s tag next,
  narrow <= wide -> checked_advance narrow s tag = Some next ->
  checked_advance wide s tag = Some next.
Proof.
  intros narrow wide s tag next Width Old; unfold checked_advance in *.
  destruct (ordinal (advance s tag) <=? narrow) eqn:Fits; try discriminate.
  apply Nat.leb_le in Fits.
  assert (L : (ordinal (advance s tag) <=? wide) = true) by (apply Nat.leb_le; lia).
  now rewrite L.
Qed.

Theorem paired_cut_indices_are_in_bounds : forall count entry,
  entry < count / 2 -> 2 * entry < count /\ 2 * entry + 1 < count.
Proof.
  intros count entry Bound.
  pose proof (Nat.div_mod count 2 ltac:(lia)).
  nia.
Qed.

Theorem method_argument_index_is_in_bounds : forall count cut,
  1 <= cut < count -> cut - 1 < count /\ S (cut - 1) = cut.
Proof. intros; lia. Qed.

Print Assumptions widening_preserves_successful_steps.
Print Assumptions paired_cut_indices_are_in_bounds.
Print Assumptions method_argument_index_is_in_bounds.

End SubstitutionRegionOrdinals.
