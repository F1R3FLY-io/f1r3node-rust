From Stdlib Require Import Lists.List Bool.Bool Arith.PeanoNat Lia Sorting.Permutation.
From CostAccountedRho Require Import VaultBackedByteAccounting.
Import ListNotations.

Record raw_byte_receipt := {
  receipt_introduction : nat;
  receipt_transfer : nat;
  receipt_trace : nat
}.

Definition raw_byte_receipt_eq_dec : forall (left right : raw_byte_receipt),
  {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition receipt_of_event event :=
  {| receipt_introduction := introduction_byte_count event;
     receipt_transfer := sum_nat (transfer_participant_bytes event);
     receipt_trace := committed_trace_byte_count event |}.

Definition receipt_weight schedule receipt :=
  introduction_rate schedule * receipt_introduction receipt +
  transfer_rate schedule * receipt_transfer receipt +
  trace_rate schedule * receipt_trace receipt.

Theorem raw_receipt_preserves_existing_quantitative_debit : forall schedule event,
  receipt_weight schedule (receipt_of_event event) = quantitative_byte_debit schedule event.
Proof. reflexivity. Qed.

Record paired_byte_row := {
  paired_identity : nat;
  paired_authority : nat;
  paired_kind : byte_event_kind;
  paired_weight : nat;
  paired_legacy_visible : bool;
  paired_raw : option raw_byte_receipt
}.

Definition measured_byte_row schedule authority event :=
  {| paired_identity := byte_event_id event; paired_authority := authority;
     paired_kind := byte_event_kind_of event;
     paired_weight := quantitative_byte_debit schedule event;
     paired_legacy_visible := true;
     paired_raw := Some (receipt_of_event event) |}.

Definition same_paired_identity authority event row :=
  (paired_identity row =? byte_event_id event) && (paired_authority row =? authority).

Definition paired_identity_seen authority event rows := existsb (same_paired_identity authority event) rows.

Definition paired_identity_compatible authority event rows :=
  forallb (fun row =>
    if same_paired_identity authority event row then
      match paired_raw row with
      | None => false
      | Some receipt => if raw_byte_receipt_eq_dec receipt (receipt_of_event event) then true else false
      end
    else true) rows.

Definition paired_event_is_deduplicated event :=
  match byte_event_kind_of event with IntroductionEvent persistent => persistent | CommunicationEvent _ => true end.

Theorem compatible_identity_has_exact_raw_receipt : forall authority event rows row,
  paired_identity_compatible authority event rows = true ->
  In row rows -> same_paired_identity authority event row = true ->
  paired_raw row = Some (receipt_of_event event).
Proof.
  intros authority event rows row compatible included same.
  unfold paired_identity_compatible in compatible. rewrite forallb_forall in compatible.
  specialize (compatible row included). rewrite same in compatible.
  destruct (paired_raw row) as [receipt|] eqn:raw; [|discriminate].
  destruct (raw_byte_receipt_eq_dec receipt (receipt_of_event event)); [now subst|discriminate].
Qed.

Record paired_byte_state := {
  paired_generation : nat;
  paired_meter : byte_accounting_state;
  paired_rows : list paired_byte_row
}.

Inductive paired_byte_result :=
| PairedAccepted (state : paired_byte_state)
| PairedRepeated (state : paired_byte_state)
| PairedRejected (state : paired_byte_state).

Definition attempt_paired_bytes maximum schedule authority event state :=
  if paired_identity_compatible authority event (paired_rows state) then
    if paired_event_is_deduplicated event && paired_identity_seen authority event (paired_rows state) then
      PairedRepeated state
    else
      match attempt_byte_event maximum schedule (paired_meter state) event with
      | ByteRejected _ => PairedRejected state
      | ByteAccepted next => PairedAccepted
          {| paired_generation := paired_generation state; paired_meter := next;
             paired_rows := measured_byte_row schedule authority event :: paired_rows state |}
      end
  else PairedRejected state.

Theorem accepted_paired_bytes_refine_existing_meter : forall maximum schedule authority event state next,
  attempt_paired_bytes maximum schedule authority event state = PairedAccepted next ->
  attempt_byte_event maximum schedule (paired_meter state) event = ByteAccepted (paired_meter next) /\
  paired_generation next = paired_generation state /\
  paired_rows next = measured_byte_row schedule authority event :: paired_rows state.
Proof.
  intros maximum schedule authority event state next accepted. unfold attempt_paired_bytes in accepted.
  destruct (paired_identity_compatible _ _ _); [|discriminate].
  destruct (_ && _); [discriminate|]. destruct (attempt_byte_event _ _ _ _) eqn:meter;
    inversion accepted; subst; auto.
Qed.

Theorem rejected_paired_bytes_change_neither_log_nor_meter : forall maximum schedule authority event state next,
  attempt_paired_bytes maximum schedule authority event state = PairedRejected next -> next = state.
Proof.
  intros maximum schedule authority event state next rejected. unfold attempt_paired_bytes in rejected.
  destruct (paired_identity_compatible _ _ _); [|now inversion rejected].
  destruct (_ && _); [discriminate|]. destruct (attempt_byte_event _ _ _ _); inversion rejected; reflexivity.
Qed.

Theorem repeated_paired_bytes_change_neither_log_nor_meter : forall maximum schedule authority event state next,
  attempt_paired_bytes maximum schedule authority event state = PairedRepeated next -> next = state.
Proof.
  intros maximum schedule authority event state next repeated. unfold attempt_paired_bytes in repeated.
  destruct (paired_identity_compatible _ _ _); [|discriminate].
  destruct (_ && _); [now inversion repeated|]. destruct (attempt_byte_event _ _ _ _); discriminate.
Qed.

Theorem incompatible_raw_identity_is_rejected : forall maximum schedule authority event state,
  paired_identity_compatible authority event (paired_rows state) = false ->
  attempt_paired_bytes maximum schedule authority event state = PairedRejected state.
Proof. intros. unfold attempt_paired_bytes. now rewrite H. Qed.

Theorem compatible_deduplicated_retry_adds_nothing : forall maximum schedule authority event state,
  paired_identity_compatible authority event (paired_rows state) = true ->
  paired_event_is_deduplicated event = true ->
  paired_identity_seen authority event (paired_rows state) = true ->
  attempt_paired_bytes maximum schedule authority event state = PairedRepeated state.
Proof. intros. unfold attempt_paired_bytes. now rewrite H, H0, H1. Qed.

Theorem nonpersistent_occurrence_is_not_deduplicated : forall maximum schedule authority event state meter,
  paired_identity_compatible authority event (paired_rows state) = true ->
  byte_event_kind_of event = IntroductionEvent false ->
  attempt_byte_event maximum schedule (paired_meter state) event = ByteAccepted meter ->
  attempt_paired_bytes maximum schedule authority event state = PairedAccepted
    {| paired_generation := paired_generation state; paired_meter := meter;
       paired_rows := measured_byte_row schedule authority event :: paired_rows state |}.
Proof.
  intros. unfold attempt_paired_bytes. rewrite H. unfold paired_event_is_deduplicated. now rewrite H0, H1.
Qed.

Definition paired_raw_complete rows := forallb (fun row => match paired_raw row with Some _ => true | None => false end) rows.

Theorem accepted_raw_event_preserves_completeness : forall maximum schedule authority event state next,
  attempt_paired_bytes maximum schedule authority event state = PairedAccepted next ->
  paired_raw_complete (paired_rows state) = true -> paired_raw_complete (paired_rows next) = true.
Proof.
  intros. apply accepted_paired_bytes_refine_existing_meter in H. destruct H as [_ [_ rows]].
  rewrite rows. exact H0.
Qed.

Theorem legacy_only_row_cannot_claim_complete_receipts : forall rows row,
  In row rows -> paired_raw row = None -> paired_raw_complete rows = false.
Proof.
  intros rows row included absent. unfold paired_raw_complete.
  destruct (forallb _ rows) eqn:complete; [|reflexivity].
  rewrite forallb_forall in complete. specialize (complete row included). rewrite absent in complete. discriminate.
Qed.

Definition paired_legacy_entry row :=
  if paired_legacy_visible row then Some (paired_identity row, paired_authority row, paired_kind row, paired_weight row)
  else None.

Definition paired_snapshot state :=
  (map paired_legacy_entry (paired_rows state),
   map paired_raw (paired_rows state)).

Theorem paired_snapshot_preserves_occurrence_alignment : forall state index row,
  nth_error (paired_rows state) index = Some row ->
  nth_error (fst (paired_snapshot state)) index =
    Some (paired_legacy_entry row) /\
  nth_error (snd (paired_snapshot state)) index = Some (paired_raw row).
Proof. intros. unfold paired_snapshot. simpl. rewrite !nth_error_map, H. split; reflexivity. Qed.

Theorem accepted_pair_logs_advance_together : forall maximum schedule authority event state next,
  attempt_paired_bytes maximum schedule authority event state = PairedAccepted next ->
  length (fst (paired_snapshot next)) = S (length (fst (paired_snapshot state))) /\
  length (snd (paired_snapshot next)) = S (length (snd (paired_snapshot state))).
Proof.
  intros. apply accepted_paired_bytes_refine_existing_meter in H. destruct H as [_ [_ rows]].
  unfold paired_snapshot. simpl. rewrite rows. split; reflexivity.
Qed.

Theorem two_accepted_orders_preserve_meter_and_receipt_multiset :
  forall maximum schedule first_authority second_authority first second initial left middle_right right middle_left,
  attempt_paired_bytes maximum schedule first_authority first initial = PairedAccepted middle_left ->
  attempt_paired_bytes maximum schedule second_authority second middle_left = PairedAccepted left ->
  attempt_paired_bytes maximum schedule second_authority second initial = PairedAccepted middle_right ->
  attempt_paired_bytes maximum schedule first_authority first middle_right = PairedAccepted right ->
  spent_debit (paired_meter left) = spent_debit (paired_meter right) /\
  committed_event_count (paired_meter left) = committed_event_count (paired_meter right) /\
  Permutation (paired_rows left) (paired_rows right).
Proof.
  intros maximum schedule first_authority second_authority first second initial left middle_right right middle_left a b c d.
  apply accepted_paired_bytes_refine_existing_meter in a, b, c, d.
  destruct a as [a [_ al]]. destruct b as [b [_ bl]]. destruct c as [c [_ cl]]. destruct d as [d [_ dl]].
  apply accepted_byte_event_is_exact in a, b, c, d. destruct a as [_ [aspend acount]].
  destruct b as [_ [bspend bcount]]. destruct c as [_ [cspend ccount]]. destruct d as [_ [dspend dcount]].
  split; [lia|]. split; [lia|]. rewrite bl, dl, al, cl. apply perm_swap.
Qed.

Definition reset_paired_bytes reserve state :=
  {| paired_generation := S (paired_generation state);
     paired_meter := {| reserved_debit := reserve; spent_debit := 0; committed_event_count := 0 |};
     paired_rows := [] |}.

Theorem reset_clears_raw_weighted_and_identity_history : forall reserve state authority event,
  paired_snapshot (reset_paired_bytes reserve state) = ([], []) /\
  paired_identity_seen authority event (paired_rows (reset_paired_bytes reserve state)) = false /\
  paired_generation (reset_paired_bytes reserve state) = S (paired_generation state) /\
  spent_debit (paired_meter (reset_paired_bytes reserve state)) = 0.
Proof. intros. repeat split; reflexivity. Qed.

Theorem zero_tariff_does_not_erase_raw_occurrence : forall authority event,
  let zero_schedule := {| introduction_rate := 0; transfer_rate := 0; trace_rate := 0 |} in
  paired_weight (measured_byte_row zero_schedule authority event) = 0 /\
  paired_raw (measured_byte_row zero_schedule authority event) = Some (receipt_of_event event).
Proof. intros. split; reflexivity. Qed.

Theorem positive_v1_receipt_cannot_have_zero_weight : forall event,
  quantitative_byte_debit byte_cost_schedule_v1 event = 0 ->
  receipt_introduction (receipt_of_event event) = 0 /\
  receipt_transfer (receipt_of_event event) = 0 /\ receipt_trace (receipt_of_event event) = 0.
Proof. intros. rewrite v1_debit_is_canonical_encoded_footprint in H. unfold receipt_of_event. simpl. lia. Qed.

Definition raw_only_byte_row authority event :=
  {| paired_identity := byte_event_id event; paired_authority := authority;
     paired_kind := byte_event_kind_of event;
     paired_weight := quantitative_byte_debit byte_cost_schedule_v1 event;
     paired_legacy_visible := false; paired_raw := Some (receipt_of_event event) |}.

Definition attempt_raw_only_bytes authority event state :=
  if paired_identity_compatible authority event (paired_rows state) then
    if paired_event_is_deduplicated event && paired_identity_seen authority event (paired_rows state) then
      PairedRepeated state
    else PairedAccepted
      {| paired_generation := paired_generation state; paired_meter := paired_meter state;
         paired_rows := raw_only_byte_row authority event :: paired_rows state |}
  else PairedRejected state.

Definition paired_legacy_projection rows := map paired_legacy_entry (filter paired_legacy_visible rows).

Theorem raw_only_acceptance_retains_receipt_without_legacy_charge : forall authority event state next,
  attempt_raw_only_bytes authority event state = PairedAccepted next ->
  paired_meter next = paired_meter state /\
  paired_generation next = paired_generation state /\
  paired_rows next = raw_only_byte_row authority event :: paired_rows state /\
  paired_legacy_projection (paired_rows next) = paired_legacy_projection (paired_rows state) /\
  snd (paired_snapshot next) = Some (receipt_of_event event) :: snd (paired_snapshot state).
Proof.
  intros authority event state next accepted. unfold attempt_raw_only_bytes in accepted.
  destruct (paired_identity_compatible _ _ _); [|discriminate].
  destruct (_ && _); [discriminate|]. inversion accepted; subst next.
  repeat split; reflexivity.
Qed.

Theorem raw_only_retry_is_atomic : forall authority event state next,
  (attempt_raw_only_bytes authority event state = PairedRepeated next \/
   attempt_raw_only_bytes authority event state = PairedRejected next) -> next = state.
Proof.
  intros authority event state next attempted. unfold attempt_raw_only_bytes in attempted.
  destruct (paired_identity_compatible _ _ _); [destruct (_ && _)|];
    destruct attempted as [attempted|attempted]; inversion attempted; reflexivity.
Qed.

Theorem v1_zero_weight_iff_all_raw_dimensions_zero : forall event,
  quantitative_byte_debit byte_cost_schedule_v1 event = 0 <->
  receipt_introduction (receipt_of_event event) = 0 /\
  receipt_transfer (receipt_of_event event) = 0 /\ receipt_trace (receipt_of_event event) = 0.
Proof.
  intros. split; [apply positive_v1_receipt_cannot_have_zero_weight|].
  intros [intro [transfer trace]]. rewrite v1_debit_is_canonical_encoded_footprint.
  unfold receipt_of_event in *. simpl in *. lia.
Qed.

Theorem raw_only_and_charged_concurrent_orders_agree :
  forall maximum schedule charged_authority raw_authority charged raw initial mid_left left mid_right right,
  attempt_paired_bytes maximum schedule charged_authority charged initial = PairedAccepted mid_left ->
  attempt_raw_only_bytes raw_authority raw mid_left = PairedAccepted left ->
  attempt_raw_only_bytes raw_authority raw initial = PairedAccepted mid_right ->
  attempt_paired_bytes maximum schedule charged_authority charged mid_right = PairedAccepted right ->
  paired_meter left = paired_meter right /\ Permutation (paired_rows left) (paired_rows right).
Proof.
  intros maximum schedule charged_authority raw_authority charged raw initial mid_left left mid_right right a b c d.
  apply accepted_paired_bytes_refine_existing_meter in a, d.
  apply raw_only_acceptance_retains_receipt_without_legacy_charge in b, c.
  destruct a as [a [_ al]]. destruct d as [d [_ dl]].
  destruct b as [bm [_ [bl _]]]. destruct c as [cm [_ [cl _]]].
  split.
  - rewrite cm in d. rewrite a in d. inversion d. congruence.
  - rewrite bl, dl, al, cl. apply perm_swap.
Qed.

Record receipt_lifecycle := {
  receipt_live_state : paired_byte_state;
  receipt_measured_context : bool;
  receipt_history_intact : bool;
  receipt_identity_metadata : list paired_byte_row
}.

Record owned_receipt_snapshot := {
  owned_receipt_rows : list paired_byte_row;
  owned_receipt_context : bool;
  owned_receipt_history : bool
}.

Definition capture_receipt_snapshot state :=
  {| owned_receipt_rows := paired_rows (receipt_live_state state);
     owned_receipt_context := receipt_measured_context state;
     owned_receipt_history := receipt_history_intact state |}.

Definition receipt_snapshot_complete snapshot :=
  owned_receipt_context snapshot && owned_receipt_history snapshot &&
  paired_raw_complete (owned_receipt_rows snapshot).

Definition partial_clear_receipt_history state :=
  {| receipt_live_state :=
       {| paired_generation := paired_generation (receipt_live_state state);
          paired_meter := paired_meter (receipt_live_state state); paired_rows := [] |};
     receipt_measured_context := receipt_measured_context state;
     receipt_history_intact := receipt_history_intact state &&
       (match paired_rows (receipt_live_state state) with [] => true | _ => false end) &&
       (match receipt_identity_metadata state with [] => true | _ => false end);
     receipt_identity_metadata := receipt_identity_metadata state |}.

Definition mark_unmeasured_direct_comm state :=
  {| receipt_live_state := receipt_live_state state;
     receipt_measured_context := receipt_measured_context state;
     receipt_history_intact := false;
     receipt_identity_metadata := receipt_identity_metadata state |}.

Definition append_lifecycle_receipt row next_meter state :=
  {| receipt_live_state :=
       {| paired_generation := paired_generation (receipt_live_state state);
          paired_meter := next_meter;
          paired_rows := row :: paired_rows (receipt_live_state state) |};
     receipt_measured_context := receipt_measured_context state;
     receipt_history_intact := receipt_history_intact state;
     receipt_identity_metadata :=
       match paired_kind row with
       | IntroductionEvent true => row :: receipt_identity_metadata state
       | _ => receipt_identity_metadata state
       end |}.

Definition quiescent_reset_receipt_lifecycle reserve state :=
  {| receipt_live_state := reset_paired_bytes reserve (receipt_live_state state);
     receipt_measured_context := receipt_measured_context state;
     receipt_history_intact := true;
     receipt_identity_metadata := [] |}.

Theorem receipt_snapshot_completeness_requires_all_three_conditions : forall snapshot,
  receipt_snapshot_complete snapshot = true <->
  owned_receipt_context snapshot = true /\ owned_receipt_history snapshot = true /\
  forall row, In row (owned_receipt_rows snapshot) -> exists raw, paired_raw row = Some raw.
Proof.
  intros snapshot. unfold receipt_snapshot_complete, paired_raw_complete.
  rewrite !andb_true_iff, forallb_forall.
  assert ((forall row, In row (owned_receipt_rows snapshot) ->
    match paired_raw row with Some _ => true | None => false end = true) <->
    (forall row, In row (owned_receipt_rows snapshot) -> exists raw, paired_raw row = Some raw)).
  { split; intros rows row included; specialize (rows row included).
    - destruct (paired_raw row); [eauto|discriminate].
    - destruct rows as [raw present]. now rewrite present. }
  tauto.
Qed.

Theorem partial_clear_preserves_identity_metadata_but_invalidates_snapshot : forall state,
  paired_rows (receipt_live_state state) <> [] \/ receipt_identity_metadata state <> [] \/
    receipt_history_intact state = false ->
  receipt_identity_metadata (partial_clear_receipt_history state) = receipt_identity_metadata state /\
  paired_rows (receipt_live_state (partial_clear_receipt_history state)) = [] /\
  receipt_snapshot_complete (capture_receipt_snapshot (partial_clear_receipt_history state)) = false.
Proof.
  intros state lost. repeat split; try reflexivity.
  unfold receipt_snapshot_complete, capture_receipt_snapshot, partial_clear_receipt_history.
  simpl. destruct (paired_rows (receipt_live_state state)) eqn:rows;
    destruct (receipt_identity_metadata state) eqn:metadata; simpl;
    try now rewrite !andb_false_r.
  destruct lost as [nonempty|[nonempty|incomplete]]; try contradiction.
  now rewrite incomplete, andb_false_r.
Qed.

Theorem empty_partial_clear_preserves_initial_completeness : forall state,
  paired_rows (receipt_live_state state) = [] -> receipt_identity_metadata state = [] ->
  receipt_snapshot_complete (capture_receipt_snapshot (partial_clear_receipt_history state)) =
    receipt_snapshot_complete (capture_receipt_snapshot state).
Proof.
  intros state rows metadata.
  unfold receipt_snapshot_complete, capture_receipt_snapshot, partial_clear_receipt_history.
  simpl. rewrite rows, metadata. simpl. now rewrite !andb_true_r.
Qed.

Theorem missing_measurement_context_cannot_certify_snapshot : forall state,
  receipt_measured_context state = false -> receipt_snapshot_complete (capture_receipt_snapshot state) = false.
Proof. intros. unfold receipt_snapshot_complete, capture_receipt_snapshot. simpl. now rewrite H. Qed.

Theorem unmeasured_direct_comm_invalidates_snapshot_without_a_row : forall state,
  paired_rows (receipt_live_state (mark_unmeasured_direct_comm state)) = paired_rows (receipt_live_state state) /\
  receipt_snapshot_complete (capture_receipt_snapshot (mark_unmeasured_direct_comm state)) = false.
Proof.
  intros. split; [reflexivity|]. unfold receipt_snapshot_complete, capture_receipt_snapshot, mark_unmeasured_direct_comm.
  simpl. now rewrite andb_false_r.
Qed.

Theorem appended_receipt_cannot_restore_lost_history : forall state row next_meter,
  receipt_history_intact state = false ->
  receipt_snapshot_complete (capture_receipt_snapshot (append_lifecycle_receipt row next_meter state)) = false.
Proof.
  intros. unfold receipt_snapshot_complete, capture_receipt_snapshot, append_lifecycle_receipt.
  simpl. now rewrite H, andb_false_r.
Qed.

Theorem unmeasured_row_prevents_complete_snapshot : forall state row,
  In row (paired_rows (receipt_live_state state)) -> paired_raw row = None ->
  receipt_snapshot_complete (capture_receipt_snapshot state) = false.
Proof.
  intros. unfold receipt_snapshot_complete, capture_receipt_snapshot. simpl.
  rewrite (legacy_only_row_cannot_claim_complete_receipts _ _ H H0). now rewrite andb_false_r.
Qed.

Theorem full_reset_restores_history_not_measurement_context : forall reserve state,
  receipt_history_intact (quiescent_reset_receipt_lifecycle reserve state) = true /\
  receipt_identity_metadata (quiescent_reset_receipt_lifecycle reserve state) = [] /\
  receipt_snapshot_complete (capture_receipt_snapshot (quiescent_reset_receipt_lifecycle reserve state)) =
    receipt_measured_context state.
Proof.
  intros. repeat split; try reflexivity.
  unfold receipt_snapshot_complete, capture_receipt_snapshot, quiescent_reset_receipt_lifecycle.
  simpl. now rewrite !andb_true_r.
Qed.

Theorem append_preserves_captured_rows_as_immutable_suffix : forall state snapshot row next_meter,
  snapshot = capture_receipt_snapshot state ->
  owned_receipt_rows (capture_receipt_snapshot (append_lifecycle_receipt row next_meter state)) =
    row :: owned_receipt_rows snapshot /\
  owned_receipt_context (capture_receipt_snapshot (append_lifecycle_receipt row next_meter state)) = owned_receipt_context snapshot /\
  owned_receipt_history (capture_receipt_snapshot (append_lifecycle_receipt row next_meter state)) = owned_receipt_history snapshot.
Proof. intros state snapshot row next_meter captured. subst snapshot. repeat split; reflexivity. Qed.

Theorem prior_owned_receipt_survives_live_partial_clear : forall state snapshot row,
  snapshot = capture_receipt_snapshot state ->
  In row (paired_rows (receipt_live_state state)) ->
  In row (owned_receipt_rows snapshot) /\
  ~ In row (owned_receipt_rows (capture_receipt_snapshot (partial_clear_receipt_history state))).
Proof. intros state snapshot row captured included. subst snapshot. simpl. tauto. Qed.

Print Assumptions receipt_snapshot_completeness_requires_all_three_conditions.
Print Assumptions partial_clear_preserves_identity_metadata_but_invalidates_snapshot.
Print Assumptions empty_partial_clear_preserves_initial_completeness.
Print Assumptions missing_measurement_context_cannot_certify_snapshot.
Print Assumptions unmeasured_direct_comm_invalidates_snapshot_without_a_row.
Print Assumptions appended_receipt_cannot_restore_lost_history.
Print Assumptions unmeasured_row_prevents_complete_snapshot.
Print Assumptions full_reset_restores_history_not_measurement_context.
Print Assumptions append_preserves_captured_rows_as_immutable_suffix.
Print Assumptions prior_owned_receipt_survives_live_partial_clear.
Print Assumptions raw_receipt_preserves_existing_quantitative_debit.
Print Assumptions accepted_paired_bytes_refine_existing_meter.
Print Assumptions rejected_paired_bytes_change_neither_log_nor_meter.
Print Assumptions repeated_paired_bytes_change_neither_log_nor_meter.
Print Assumptions incompatible_raw_identity_is_rejected.
Print Assumptions compatible_deduplicated_retry_adds_nothing.
Print Assumptions nonpersistent_occurrence_is_not_deduplicated.
Print Assumptions accepted_raw_event_preserves_completeness.
Print Assumptions legacy_only_row_cannot_claim_complete_receipts.
Print Assumptions paired_snapshot_preserves_occurrence_alignment.
Print Assumptions accepted_pair_logs_advance_together.
Print Assumptions two_accepted_orders_preserve_meter_and_receipt_multiset.
Print Assumptions reset_clears_raw_weighted_and_identity_history.
Print Assumptions zero_tariff_does_not_erase_raw_occurrence.
Print Assumptions positive_v1_receipt_cannot_have_zero_weight.
Print Assumptions raw_only_acceptance_retains_receipt_without_legacy_charge.
Print Assumptions raw_only_retry_is_atomic.
Print Assumptions v1_zero_weight_iff_all_raw_dimensions_zero.
Print Assumptions compatible_identity_has_exact_raw_receipt.
Print Assumptions raw_only_and_charged_concurrent_orders_agree.
