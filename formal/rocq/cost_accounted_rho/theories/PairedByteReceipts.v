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

Theorem reconstructed_native_observation_retains_actual_fields : forall authority event,
  paired_identity (raw_only_byte_row authority event) = byte_event_id event /\
  paired_authority (raw_only_byte_row authority event) = authority /\
  paired_kind (raw_only_byte_row authority event) = byte_event_kind_of event /\
  paired_raw (raw_only_byte_row authority event) = Some (receipt_of_event event) /\
  paired_legacy_entry (raw_only_byte_row authority event) = None.
Proof. intros. repeat split. Qed.

Theorem native_and_legacy_construction_preserve_same_measurement : forall schedule authority event,
  paired_identity (raw_only_byte_row authority event) = paired_identity (measured_byte_row schedule authority event) /\
  paired_authority (raw_only_byte_row authority event) = paired_authority (measured_byte_row schedule authority event) /\
  paired_kind (raw_only_byte_row authority event) = paired_kind (measured_byte_row schedule authority event) /\
  paired_raw (raw_only_byte_row authority event) = paired_raw (measured_byte_row schedule authority event).
Proof. intros. repeat split. Qed.

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

Definition empty_raw_receipt :=
  {| receipt_introduction := 0; receipt_transfer := 0; receipt_trace := 0 |}.

Definition add_raw_receipts left right :=
  {| receipt_introduction := receipt_introduction left + receipt_introduction right;
     receipt_transfer := receipt_transfer left + receipt_transfer right;
     receipt_trace := receipt_trace left + receipt_trace right |}.

Definition raw_row_dimensions row :=
  match paired_raw row with Some raw => raw | None => empty_raw_receipt end.

Fixpoint raw_rows_total rows :=
  match rows with
  | [] => empty_raw_receipt
  | row :: tail => add_raw_receipts (raw_row_dimensions row) (raw_rows_total tail)
  end.

Definition raw_total_fits maximum total :=
  (receipt_introduction total <=? maximum) &&
  (receipt_transfer total <=? maximum) &&
  (receipt_trace total <=? maximum).

Definition capture_counted_receipts maximum entries snapshot :=
  let rows := owned_receipt_rows snapshot in
  let total := raw_rows_total rows in
  if receipt_snapshot_complete snapshot && (length rows <=? entries) && raw_total_fits maximum total
  then Some (rows, total) else None.

Theorem counted_receipts_preserve_all_rows : forall maximum entries snapshot rows total,
  capture_counted_receipts maximum entries snapshot = Some (rows, total) ->
  rows = owned_receipt_rows snapshot /\ total = raw_rows_total rows.
Proof.
  intros. unfold capture_counted_receipts in H. destruct (_ && _); inversion H; auto.
Qed.

Theorem counted_receipts_require_complete_snapshot : forall maximum entries snapshot rows total,
  capture_counted_receipts maximum entries snapshot = Some (rows, total) ->
  receipt_snapshot_complete snapshot = true.
Proof.
  intros. unfold capture_counted_receipts in H.
  destruct (_ && _) eqn:accepted; [|discriminate].
  apply andb_true_iff in accepted as [prefix _].
  apply andb_true_iff in prefix as [complete _]. exact complete.
Qed.

Theorem counted_receipts_require_every_raw_occurrence : forall maximum entries snapshot rows total row,
  capture_counted_receipts maximum entries snapshot = Some (rows, total) ->
  In row rows -> exists raw, paired_raw row = Some raw.
Proof.
  intros maximum entries snapshot rows total row accepted included.
  pose proof (counted_receipts_require_complete_snapshot _ _ _ _ _ accepted) as complete.
  apply receipt_snapshot_completeness_requires_all_three_conditions in complete as [_ [_ present]].
  pose proof (counted_receipts_preserve_all_rows _ _ _ _ _ accepted) as [same _].
  subst rows. now apply present.
Qed.

Theorem counted_receipts_bound_every_dimension : forall maximum entries snapshot rows total,
  capture_counted_receipts maximum entries snapshot = Some (rows, total) ->
  length rows <= entries /\ receipt_introduction total <= maximum /\
  receipt_transfer total <= maximum /\ receipt_trace total <= maximum.
Proof.
  intros. unfold capture_counted_receipts in H.
  destruct (_ && _) eqn:accepted; [|discriminate]. inversion H; subst.
  unfold raw_total_fits in accepted.
  repeat rewrite andb_true_iff in accepted.
  repeat rewrite Nat.leb_le in accepted. tauto.
Qed.

Theorem raw_receipt_totals_append : forall left right,
  raw_rows_total (left ++ right) = add_raw_receipts (raw_rows_total left) (raw_rows_total right).
Proof.
  induction left as [|head tail IH]; intros right; simpl.
  - destruct (raw_rows_total right); reflexivity.
  - rewrite IH. unfold add_raw_receipts. simpl. f_equal; lia.
Qed.

Theorem raw_receipt_totals_permutation : forall left right,
  Permutation left right -> raw_rows_total left = raw_rows_total right.
Proof.
  intros left right same. induction same; simpl.
  - reflexivity.
  - now rewrite IHsame.
  - unfold add_raw_receipts. simpl. f_equal; lia.
  - congruence.
Qed.

Theorem raw_receipt_totals_keep_repeated_occurrences : forall row rows,
  raw_rows_total (row :: row :: rows) =
  add_raw_receipts (raw_row_dimensions row)
    (add_raw_receipts (raw_row_dimensions row) (raw_rows_total rows)).
Proof. reflexivity. Qed.

Theorem counted_receipts_accept_complete_bounded_snapshot : forall maximum entries snapshot,
  receipt_snapshot_complete snapshot = true ->
  length (owned_receipt_rows snapshot) <= entries ->
  raw_total_fits maximum (raw_rows_total (owned_receipt_rows snapshot)) = true ->
  capture_counted_receipts maximum entries snapshot =
    Some (owned_receipt_rows snapshot, raw_rows_total (owned_receipt_rows snapshot)).
Proof.
  intros maximum entries snapshot complete bounded fits.
  unfold capture_counted_receipts. apply Nat.leb_le in bounded.
  rewrite complete, bounded, fits. reflexivity.
Qed.

Theorem bounded_raw_total_bounds_every_prefix : forall maximum prefix suffix,
  raw_total_fits maximum (raw_rows_total (prefix ++ suffix)) = true ->
  raw_total_fits maximum (raw_rows_total prefix) = true.
Proof.
  intros maximum prefix suffix fits. rewrite raw_receipt_totals_append in fits.
  unfold raw_total_fits, add_raw_receipts in *. simpl in *.
  repeat rewrite andb_true_iff in *.
  repeat rewrite Nat.leb_le in *. lia.
Qed.

Print Assumptions counted_receipts_preserve_all_rows.
Print Assumptions counted_receipts_require_complete_snapshot.
Print Assumptions counted_receipts_require_every_raw_occurrence.
Print Assumptions counted_receipts_bound_every_dimension.
Print Assumptions raw_receipt_totals_append.
Print Assumptions raw_receipt_totals_permutation.
Print Assumptions raw_receipt_totals_keep_repeated_occurrences.
Print Assumptions counted_receipts_accept_complete_bounded_snapshot.
Print Assumptions bounded_raw_total_bounds_every_prefix.

Definition native_row_quantity dimension row :=
  match dimension with
  | 0 => match paired_kind row with CommunicationEvent _ => 1 | _ => 0 end
  | 1 => receipt_introduction (raw_row_dimensions row)
  | 2 => receipt_transfer (raw_row_dimensions row)
  | 3 => receipt_trace (raw_row_dimensions row)
  | _ => 0
  end.

Record native_measurement_occurrence := {
  native_occurrence_row : paired_byte_row;
  native_occurrence_dimension : nat;
  native_occurrence_quantity : nat
}.

Definition project_native_row row :=
  filter (fun occurrence => 0 <? native_occurrence_quantity occurrence)
    (map (fun dimension =>
      {| native_occurrence_row := row;
         native_occurrence_dimension := dimension;
         native_occurrence_quantity := native_row_quantity dimension row |}) (seq 0 4)).

Definition project_native_rows rows := flat_map project_native_row rows.

Definition native_dimension_total dimension rows :=
  fold_right (fun row total => native_row_quantity dimension row + total) 0 rows.

Theorem native_projection_keeps_source_and_exact_positive_quantity : forall row occurrence,
  In occurrence (project_native_row row) ->
  native_occurrence_row occurrence = row /\
  native_occurrence_dimension occurrence < 4 /\
  native_occurrence_quantity occurrence = native_row_quantity (native_occurrence_dimension occurrence) row /\
  0 < native_occurrence_quantity occurrence.
Proof.
  intros row occurrence included. unfold project_native_row in included.
  apply filter_In in included as [mapped positive].
  apply in_map_iff in mapped as [dimension [same bounded]].
  subst occurrence. apply in_seq in bounded. simpl in *.
  apply Nat.ltb_lt in positive. repeat split; auto; lia.
Qed.

Theorem native_projection_contains_every_positive_dimension : forall row dimension,
  dimension < 4 -> 0 < native_row_quantity dimension row ->
  In {| native_occurrence_row := row;
        native_occurrence_dimension := dimension;
        native_occurrence_quantity := native_row_quantity dimension row |} (project_native_row row).
Proof.
  intros row dimension bounded positive. unfold project_native_row.
  apply filter_In. split.
  - apply in_map_iff. exists dimension. split; [reflexivity|]. apply in_seq. lia.
  - simpl. now apply Nat.ltb_lt.
Qed.

Theorem native_projection_preserves_append : forall left right,
  project_native_rows (left ++ right) = project_native_rows left ++ project_native_rows right.
Proof. intros. apply flat_map_app. Qed.

Theorem native_projection_preserves_repeated_occurrences : forall row rows,
  project_native_rows (row :: row :: rows) =
    project_native_row row ++ project_native_row row ++ project_native_rows rows.
Proof. reflexivity. Qed.

Theorem positive_native_occurrence_has_positive_total : forall dimension row rows,
  In row rows -> 0 < native_row_quantity dimension row ->
  0 < native_dimension_total dimension rows.
Proof.
  intros dimension row rows. induction rows as [|head tail IH]; simpl; intros included positive.
  - contradiction.
  - destruct included as [same|included].
    + subst head. unfold native_dimension_total. simpl. lia.
    + specialize (IH included positive). unfold native_dimension_total in *. simpl. lia.
Qed.

Theorem native_dimension_totals_ignore_order : forall dimension left right,
  Permutation left right -> native_dimension_total dimension left = native_dimension_total dimension right.
Proof.
  intros dimension left right same. unfold native_dimension_total. induction same; simpl; lia.
Qed.

Theorem native_quantities_ignore_legacy_projection : forall first second dimension,
  paired_kind first = paired_kind second -> paired_raw first = paired_raw second ->
  native_row_quantity dimension first = native_row_quantity dimension second.
Proof.
  intros first second dimension kinds raw.
  unfold native_row_quantity, raw_row_dimensions. rewrite kinds, raw. reflexivity.
Qed.

Theorem native_and_legacy_construction_preserve_all_native_quantities : forall schedule authority event dimension,
  native_row_quantity dimension (raw_only_byte_row authority event) =
  native_row_quantity dimension (measured_byte_row schedule authority event).
Proof. intros. apply native_quantities_ignore_legacy_projection; reflexivity. Qed.

Print Assumptions native_projection_keeps_source_and_exact_positive_quantity.
Print Assumptions native_projection_contains_every_positive_dimension.
Print Assumptions native_projection_preserves_append.
Print Assumptions native_projection_preserves_repeated_occurrences.
Print Assumptions positive_native_occurrence_has_positive_total.
Print Assumptions native_dimension_totals_ignore_order.
Print Assumptions native_quantities_ignore_legacy_projection.

Section NativeRegionProjection.
Context {Region : Type}.
Variable regions_of : paired_byte_row -> list Region.

Definition project_native_regions row :=
  flat_map (fun occurrence => map (fun region => (occurrence, region)) (regions_of row))
    (project_native_row row).

Definition project_native_region_rows rows := flat_map project_native_regions rows.

Theorem native_regions_preserve_exact_evidence : forall row occurrence region,
  In (occurrence, region) (project_native_regions row) <->
  In occurrence (project_native_row row) /\ In region (regions_of row).
Proof.
  intros. unfold project_native_regions. rewrite in_flat_map. split.
  - intros [source [present mapped]]. apply in_map_iff in mapped as [r [same included]].
    inversion same; subst. auto.
  - intros [present included]. exists occurrence. split; [assumption|].
    apply in_map. assumption.
Qed.

Theorem native_regions_keep_source_dimension_and_quantity : forall row occurrence region,
  In (occurrence, region) (project_native_regions row) ->
  native_occurrence_row occurrence = row /\
  native_occurrence_dimension occurrence < 4 /\
  native_occurrence_quantity occurrence = native_row_quantity (native_occurrence_dimension occurrence) row /\
  0 < native_occurrence_quantity occurrence /\ In region (regions_of row).
Proof.
  intros row occurrence region included.
  apply native_regions_preserve_exact_evidence in included as [present included].
  apply native_projection_keeps_source_and_exact_positive_quantity in present. tauto.
Qed.

Theorem native_regions_include_every_positive_dimension : forall row dimension region,
  dimension < 4 -> 0 < native_row_quantity dimension row -> In region (regions_of row) ->
  In ({| native_occurrence_row := row;
         native_occurrence_dimension := dimension;
         native_occurrence_quantity := native_row_quantity dimension row |}, region)
    (project_native_regions row).
Proof.
  intros. apply native_regions_preserve_exact_evidence. split; [|assumption].
  now apply native_projection_contains_every_positive_dimension.
Qed.

Theorem native_regions_preserve_multiplicity : forall row,
  length (project_native_regions row) =
  length (project_native_row row) * length (regions_of row).
Proof.
  intros row. unfold project_native_regions.
  induction (project_native_row row) as [|head tail IH]; simpl; [reflexivity|].
  rewrite length_app, length_map, IH. reflexivity.
Qed.

Theorem native_region_projection_preserves_append : forall left right,
  project_native_region_rows (left ++ right) =
  project_native_region_rows left ++ project_native_region_rows right.
Proof. intros. apply flat_map_app. Qed.

Theorem native_region_projection_preserves_repeated_occurrences : forall row rows,
  project_native_region_rows (row :: row :: rows) =
  project_native_regions row ++ project_native_regions row ++ project_native_region_rows rows.
Proof. reflexivity. Qed.

Theorem native_region_projection_preserves_permutation : forall left right,
  Permutation left right ->
  Permutation (project_native_region_rows left) (project_native_region_rows right).
Proof.
  intros left right same. induction same; unfold project_native_region_rows in *; simpl in *.
  - apply Permutation_refl.
  - now apply Permutation_app_head.
  - rewrite !app_assoc. apply Permutation_app_tail. apply Permutation_app_comm.
  - eapply Permutation_trans; eassumption.
Qed.
End NativeRegionProjection.

Print Assumptions native_regions_preserve_exact_evidence.
Print Assumptions native_regions_keep_source_dimension_and_quantity.
Print Assumptions native_regions_include_every_positive_dimension.
Print Assumptions native_regions_preserve_multiplicity.
Print Assumptions native_region_projection_preserves_append.
Print Assumptions native_region_projection_preserves_repeated_occurrences.
Print Assumptions native_region_projection_preserves_permutation.

Section NativePurseProjection.
Context {Region Channel : Type}.
Variable channel_of : Region -> Channel.

Definition locate_native_demands (demands : list (native_measurement_occurrence * Region)) :=
  map (fun demand => (demand, channel_of (snd demand))) demands.

Theorem native_purse_projection_keeps_complete_evidence : forall demands,
  map fst (locate_native_demands demands) = demands.
Proof. induction demands; simpl; congruence. Qed.

Theorem native_purse_projection_keeps_every_occurrence : forall demands,
  length (locate_native_demands demands) = length demands.
Proof. intros. apply length_map. Qed.

Theorem native_purse_aliases_do_not_merge_demands : forall left right rest,
  channel_of (snd left) = channel_of (snd right) ->
  locate_native_demands (left :: right :: rest) =
  (left, channel_of (snd left)) :: (right, channel_of (snd right)) :: locate_native_demands rest.
Proof. reflexivity. Qed.

Theorem native_purse_projection_preserves_permutation : forall left right,
  Permutation left right ->
  Permutation (locate_native_demands left) (locate_native_demands right).
Proof. intros. now apply Permutation_map. Qed.

Theorem native_purse_binding_preserves_original_region : forall demands occurrence region channel,
  In ((occurrence, region), channel) (locate_native_demands demands) ->
  In (occurrence, region) demands /\ channel = channel_of region.
Proof.
  intros demands occurrence region channel included.
  apply in_map_iff in included as [[source owner] [same present]].
  simpl in same. inversion same. subst. auto.
Qed.
End NativePurseProjection.

Print Assumptions native_purse_projection_keeps_complete_evidence.
Print Assumptions native_purse_projection_keeps_every_occurrence.
Print Assumptions native_purse_aliases_do_not_merge_demands.
Print Assumptions native_purse_projection_preserves_permutation.
Print Assumptions native_purse_binding_preserves_original_region.

Section NativeAcquisitionDemand.
Context {Region Location Authority Terms Class : Type}.
Variable location_of : Region -> Location.
Variable authority_of : Region -> Authority.
Variable class_of : nat -> Class.

Record native_acquisition_demand := {
  acquisition_source : native_measurement_occurrence * Region;
  acquisition_location : Location;
  acquisition_class : Class;
  acquisition_terms : Terms;
  acquisition_authority : Authority;
  acquisition_quantity : nat
}.

Definition acquire_native_demand terms (source : native_measurement_occurrence * Region) :=
  {| acquisition_source := source;
     acquisition_location := location_of (snd source);
     acquisition_class := class_of (native_occurrence_dimension (fst source));
     acquisition_terms := terms;
     acquisition_authority := authority_of (snd source);
     acquisition_quantity := native_occurrence_quantity (fst source) |}.

Definition acquire_native_demands terms sources := map (acquire_native_demand terms) sources.

Theorem native_acquisition_keeps_complete_evidence : forall terms sources,
  map acquisition_source (acquire_native_demands terms sources) = sources.
Proof. induction sources; simpl; congruence. Qed.

Theorem native_acquisition_keeps_exact_resource : forall terms source,
  let resource := acquire_native_demand terms source in
  acquisition_location resource = location_of (snd source) /\
  acquisition_class resource = class_of (native_occurrence_dimension (fst source)) /\
  acquisition_terms resource = terms /\
  acquisition_authority resource = authority_of (snd source) /\
  acquisition_quantity resource = native_occurrence_quantity (fst source).
Proof. intros. repeat split; reflexivity. Qed.

Theorem native_acquisition_preserves_occurrence_count : forall terms sources,
  length (acquire_native_demands terms sources) = length sources.
Proof. intros. apply length_map. Qed.

Theorem native_acquisition_preserves_permutation : forall terms left right,
  Permutation left right ->
  Permutation (acquire_native_demands terms left) (acquire_native_demands terms right).
Proof. intros. now apply Permutation_map. Qed.

Theorem native_acquisition_preserves_append : forall terms left right,
  acquire_native_demands terms (left ++ right) =
  acquire_native_demands terms left ++ acquire_native_demands terms right.
Proof. intros. apply map_app. Qed.

Theorem native_acquisition_terms_do_not_change_measurement : forall old_terms new_terms sources,
  map acquisition_quantity (acquire_native_demands old_terms sources) =
  map acquisition_quantity (acquire_native_demands new_terms sources).
Proof. induction sources; simpl; congruence. Qed.

Variable weight : Class -> nat.
Variable authority_value : Authority -> nat.

Definition acquisition_usage resource :=
  acquisition_quantity resource * weight (acquisition_class resource) *
  authority_value (acquisition_authority resource).

Definition measured_authority_usage source :=
  native_occurrence_quantity (fst source) *
  weight (class_of (native_occurrence_dimension (fst source))) *
  authority_value (authority_of (snd source)).

Theorem native_acquisition_preserves_weighted_usage : forall terms sources,
  fold_right (fun resource total => acquisition_usage resource + total) 0
    (acquire_native_demands terms sources) =
  fold_right (fun source total => measured_authority_usage source + total) 0 sources.
Proof.
  intros terms sources. induction sources; simpl; [reflexivity|].
  rewrite IHsources. unfold acquisition_usage, measured_authority_usage. reflexivity.
Qed.

Theorem native_acquisition_bound_includes_every_prefix : forall terms prefix suffix bound,
  fold_right (fun resource total => acquisition_usage resource + total) 0
    (acquire_native_demands terms (prefix ++ suffix)) <= bound ->
  fold_right (fun resource total => acquisition_usage resource + total) 0
    (acquire_native_demands terms prefix) <= bound.
Proof.
  intros terms prefix. induction prefix as [|head tail IH]; simpl; intros suffix bound fits.
  - lia.
  - specialize (IH suffix (bound - acquisition_usage (acquire_native_demand terms head))).
    assert (enough : acquisition_usage (acquire_native_demand terms head) <= bound) by lia.
    assert (rest : fold_right (fun resource total => acquisition_usage resource + total) 0
      (acquire_native_demands terms tail) <=
      bound - acquisition_usage (acquire_native_demand terms head)).
    { apply IH. lia. }
    lia.
Qed.
End NativeAcquisitionDemand.

Print Assumptions native_acquisition_keeps_complete_evidence.
Print Assumptions native_acquisition_keeps_exact_resource.
Print Assumptions native_acquisition_preserves_occurrence_count.
Print Assumptions native_acquisition_preserves_permutation.
Print Assumptions native_acquisition_preserves_append.
Print Assumptions native_acquisition_terms_do_not_change_measurement.
Print Assumptions native_acquisition_preserves_weighted_usage.
Print Assumptions native_acquisition_bound_includes_every_prefix.

Definition native_live_reservation limit used charge :=
  if used + charge <=? limit then Some (used + charge) else None.

Theorem native_live_reservation_exact : forall limit used charge next,
  native_live_reservation limit used charge = Some next ->
  next = used + charge /\ next <= limit.
Proof.
  intros limit used charge next accepted. unfold native_live_reservation in accepted.
  destruct (used + charge <=? limit) eqn:fits; [|discriminate].
  apply Nat.leb_le in fits. inversion accepted. auto.
Qed.

Theorem native_live_reservation_complete : forall limit used charge,
  used + charge <= limit ->
  native_live_reservation limit used charge = Some (used + charge).
Proof.
  intros limit used charge fits. unfold native_live_reservation.
  now rewrite (proj2 (Nat.leb_le _ _) fits).
Qed.

Theorem native_live_reservation_rejects_excess : forall limit used charge,
  native_live_reservation limit used charge = None <-> limit < used + charge.
Proof.
  intros. unfold native_live_reservation. destruct (_ <=? _) eqn:fits.
  - apply Nat.leb_le in fits. split; [discriminate|lia].
  - apply Nat.leb_gt in fits. split; auto.
Qed.

Theorem native_live_reservation_zero_is_valid_at_ceiling : forall limit,
  native_live_reservation limit limit 0 = Some limit.
Proof. intros. unfold native_live_reservation. rewrite Nat.add_0_r, Nat.leb_refl. reflexivity. Qed.

Theorem native_live_reservations_compose : forall limit used first middle second final,
  native_live_reservation limit used first = Some middle ->
  native_live_reservation limit middle second = Some final ->
  native_live_reservation limit used (first + second) = Some final.
Proof.
  intros limit used first middle second final one two.
  apply native_live_reservation_exact in one, two. destruct one as [-> _].
  destruct two as [-> fits].
  replace (used + first + second) with (used + (first + second)) by lia.
  apply native_live_reservation_complete. lia.
Qed.

Theorem native_live_reservations_commute_when_jointly_funded : forall limit used first second,
  used + first + second <= limit ->
  native_live_reservation limit used first = Some (used + first) /\
  native_live_reservation limit (used + first) second = Some (used + first + second) /\
  native_live_reservation limit used second = Some (used + second) /\
  native_live_reservation limit (used + second) first = Some (used + first + second).
Proof.
  intros limit used first second fits. repeat split;
    try (apply native_live_reservation_complete; lia).
  replace (used + first + second) with (used + second + first) by lia.
  apply native_live_reservation_complete. lia.
Qed.

Theorem native_live_reservation_bounded_machine_refinement : forall maximum limit used charge next,
  limit <= maximum ->
  native_live_reservation limit used charge = Some next ->
  used <= maximum /\ charge <= maximum /\ next <= maximum.
Proof.
  intros maximum limit used charge next bound accepted.
  apply native_live_reservation_exact in accepted. destruct accepted. lia.
Qed.

Definition native_dimension_region_usage weight quantity values :=
  fold_right (fun value total => weight * value * quantity + total) 0 values.

Theorem native_dimension_region_usage_exact : forall weight quantity values,
  native_dimension_region_usage weight quantity values =
  weight * fold_right Nat.add 0 values * quantity.
Proof.
  intros weight quantity values. induction values as [|value tail IH]; simpl.
  - unfold native_dimension_region_usage. simpl. lia.
  - unfold native_dimension_region_usage in *. simpl in *. rewrite IH. nia.
Qed.

Theorem native_dimension_region_usage_preserves_duplicates : forall weight quantity value values,
  native_dimension_region_usage weight quantity (value :: value :: values) =
  2 * (weight * value * quantity) + native_dimension_region_usage weight quantity values.
Proof. intros. unfold native_dimension_region_usage. simpl. lia. Qed.

Print Assumptions native_live_reservation_exact.
Print Assumptions native_live_reservation_complete.
Print Assumptions native_live_reservation_rejects_excess.
Print Assumptions native_live_reservation_zero_is_valid_at_ceiling.
Print Assumptions native_live_reservations_compose.
Print Assumptions native_live_reservations_commute_when_jointly_funded.
Print Assumptions native_live_reservation_bounded_machine_refinement.
Print Assumptions native_dimension_region_usage_exact.
Print Assumptions native_dimension_region_usage_preserves_duplicates.

Section NativeObservationAcceptance.
Context {Key Row : Type}.
Variable key_eq : forall (left right : Key), {left = right} + {left <> right}.
Variable row_eq : forall (left right : Row), {left = right} + {left <> right}.
Variable charge_of : Row -> nat.

Record native_observation_state := {
  native_used : nat;
  native_rows : list (Key * Row)
}.

Fixpoint native_find (key : Key) (rows : list (Key * Row)) : option Row :=
  match rows with
  | [] => None
  | (stored_key, row) :: tail => if key_eq key stored_key then Some row else native_find key tail
  end.

Definition native_append limit key row state :=
  match native_live_reservation limit (native_used state) (charge_of row) with
  | None => None
  | Some next => Some {| native_used := next; native_rows := (key, row) :: native_rows state |}
  end.

Definition native_accept limit (idempotent : bool) key row state :=
  if idempotent then
    match native_find key (native_rows state) with
    | Some stored => if row_eq row stored then Some state else None
    | None => native_append limit key row state
    end
  else native_append limit key row state.

Definition native_ledger_usage (rows : list (Key * Row)) :=
  fold_right (fun entry total => charge_of (snd entry) + total) 0 rows.

Theorem native_append_preserves_exact_receipt_and_usage : forall limit key row state next,
  native_append limit key row state = Some next ->
  native_used next = native_used state + charge_of row /\
  native_used next <= limit /\
  native_rows next = (key, row) :: native_rows state.
Proof.
  intros limit key row state next accepted. unfold native_append in accepted.
  destruct (native_live_reservation _ _ _) eqn:reserve; [|discriminate].
  apply native_live_reservation_exact in reserve. inversion accepted; subst. simpl. tauto.
Qed.

Theorem native_accept_preserves_exact_ledger : forall limit idempotent key row state next,
  native_used state = native_ledger_usage (native_rows state) ->
  native_accept limit idempotent key row state = Some next ->
  native_used next = native_ledger_usage (native_rows next).
Proof.
  intros limit idempotent key row state next exact accepted.
  unfold native_accept in accepted. destruct idempotent.
  - destruct (native_find _ _) as [stored|] eqn:found.
    + destruct (row_eq _ _); [now inversion accepted; subst|discriminate].
    + apply native_append_preserves_exact_receipt_and_usage in accepted.
      destruct accepted as [used [_ rows]]. rewrite used, rows. unfold native_ledger_usage in *. simpl. lia.
  - apply native_append_preserves_exact_receipt_and_usage in accepted.
    destruct accepted as [used [_ rows]]. rewrite used, rows. unfold native_ledger_usage in *. simpl. lia.
Qed.

Theorem native_accept_preserves_ceiling : forall limit idempotent key row state next,
  native_used state <= limit ->
  native_accept limit idempotent key row state = Some next -> native_used next <= limit.
Proof.
  intros limit idempotent key row state next bounded accepted.
  unfold native_accept in accepted. destruct idempotent.
  - destruct (native_find _ _) as [stored|].
    + destruct (row_eq _ _); [now inversion accepted; subst|discriminate].
    + now apply native_append_preserves_exact_receipt_and_usage in accepted as [_ [fits _]].
  - now apply native_append_preserves_exact_receipt_and_usage in accepted as [_ [fits _]].
Qed.

Theorem native_compatible_retry_preserves_the_entire_state : forall limit key row state,
  native_find key (native_rows state) = Some row ->
  native_accept limit true key row state = Some state.
Proof.
  intros limit key row state found. unfold native_accept. rewrite found.
  destruct (row_eq row row); congruence.
Qed.

Theorem native_incompatible_retry_rejects : forall limit key row stored state,
  native_find key (native_rows state) = Some stored -> row <> stored ->
  native_accept limit true key row state = None.
Proof.
  intros limit key row stored state found different. unfold native_accept. rewrite found.
  destruct (row_eq row stored); congruence.
Qed.

Theorem native_repeated_nonpersistent_occurrences_remain_distinct : forall limit key row state middle final,
  native_accept limit false key row state = Some middle ->
  native_accept limit false key row middle = Some final ->
  native_used final = native_used state + 2 * charge_of row /\
  native_rows final = (key, row) :: (key, row) :: native_rows state.
Proof.
  intros limit key row state middle final first second.
  unfold native_accept in first, second.
  apply native_append_preserves_exact_receipt_and_usage in first, second.
  destruct first as [one [_ rows_one]]. destruct second as [two [_ rows_two]].
  split; [lia|]. now rewrite rows_two, rows_one.
Qed.
End NativeObservationAcceptance.

Print Assumptions native_append_preserves_exact_receipt_and_usage.
Print Assumptions native_accept_preserves_exact_ledger.
Print Assumptions native_accept_preserves_ceiling.
Print Assumptions native_compatible_retry_preserves_the_entire_state.
Print Assumptions native_incompatible_retry_rejects.
Print Assumptions native_repeated_nonpersistent_occurrences_remain_distinct.
