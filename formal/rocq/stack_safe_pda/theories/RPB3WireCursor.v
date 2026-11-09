From Stdlib Require Import List Bool Arith Lia NArith.

Import ListNotations.

(** Canonical protobuf framing used by protobuf_cursor.rs:412-585 and
    indexed_protobuf.rs:1361-1405,1695-1900. Offsets are absolute positions
    in a finite backing list; [region_start, region_end) selects a subregion.
    Nat is used for byte values and offsets, and binary N for u64 varints,
    with an explicit u8 input check.
    This proof covers framing and numeric field order, not generated Prost-slot
    order, source provenance, usize overflow diagnostics, or child dispatch. *)

Inductive wire_kind := WireVarint | WireFixed64 | WireLength | WireFixed32.

Record field_span := FieldSpan {
  span_number : nat;
  span_wire : wire_kind;
  span_start : nat;
  span_key_end : nat;
  span_payload_start : nat;
  span_end : nat;
  span_varint_value : option N
}.

Definition byte_ok (byte : nat) : bool := byte <? 256.

Definition field_number_within_limit (number : N) : bool :=
  N.leb number 536870911%N.

Lemma binary_and_nat_bounds_agree : forall number bound,
  N.leb number bound = Nat.leb (N.to_nat number) (N.to_nat bound).
Proof.
  intros number bound.
  destruct (N.leb number bound) eqn:binary;
    destruct (Nat.leb (N.to_nat number) (N.to_nat bound)) eqn:unary;
    try reflexivity.
  - apply N.leb_le in binary. apply Nat.leb_gt in unary.
    apply N.compare_le_iff in binary.
    rewrite N2Nat.inj_compare in binary.
    apply Nat.compare_le_iff in binary. lia.
  - apply N.leb_gt in binary. apply Nat.leb_le in unary.
    unfold N.lt in binary.
    rewrite N2Nat.inj_compare in binary.
    apply Nat.compare_lt_iff in binary. lia.
Qed.

Theorem field_number_within_limit_exact : forall number,
  field_number_within_limit number =
    Nat.leb (N.to_nat number) (N.to_nat 536870911%N).
Proof. intro number. apply binary_and_nat_bounds_agree. Qed.

Theorem field_number_within_limit_is_29_bit : forall number,
  field_number_within_limit number = true <->
    (number < 536870912)%N.
Proof.
  intro number. unfold field_number_within_limit.
  rewrite N.leb_le. lia.
Qed.

Definition region_ok (bytes : list nat) (region_start region_end : nat) : bool :=
  (region_start <=? region_end) &&
  (region_end <=? length bytes) &&
  forallb byte_ok (firstn (region_end - region_start)
    (skipn region_start bytes)).

(** Ten groups exactly match Rust's u64 loop. Group 9 admits only 0 or 1;
    a zero terminal group after any earlier group is nonminimal. Every read
    is bounded by [region_end], including header and payload varints. *)
Fixpoint varint_groups (fuel : nat) (bytes : list nat)
    (region_end offset group : nat) (value : N) : option (N * nat) :=
  match fuel with
  | 0 => None
  | S rest =>
      if offset <? region_end then
        match nth_error bytes offset with
        | None => None
        | Some byte =>
            if byte_ok byte then
              if (group =? 9) && (1 <? byte) then None
              else
                let next_value :=
                  (value + N.of_nat (byte mod 128) * 128 ^ N.of_nat group)%N in
                if byte <? 128 then
                  if (0 <? group) && (byte =? 0) then None
                  else Some (next_value, S offset)
                else varint_groups rest bytes region_end (S offset)
                  (S group) next_value
            else None
        end
      else None
  end.

Definition canonical_varint (bytes : list nat) (region_end offset : nat)
    : option (N * nat) :=
  varint_groups 10 bytes region_end offset 0 0.

Lemma binary_varint_accumulator_step_exact : forall value byte group,
  N.to_nat (value + N.of_nat (byte mod 128) *
    128 ^ N.of_nat group)%N =
  N.to_nat value + (byte mod 128) * (128 ^ group).
Proof.
  intros value byte group.
  rewrite N2Nat.inj_add, N2Nat.inj_mul, N2Nat.inj_pow.
  now rewrite !Nat2N.id.
Qed.

(** The previous unary recurrence is retained only as a mathematical
    reference. Its large finite values are never normalized by a proof. *)
Fixpoint varint_groups_nat_spec (fuel : nat) (bytes : list nat)
    (region_end offset group value : nat) : option (nat * nat) :=
  match fuel with
  | 0 => None
  | S rest =>
      if offset <? region_end then
        match nth_error bytes offset with
        | None => None
        | Some byte =>
            if byte_ok byte then
              if (group =? 9) && (1 <? byte) then None
              else
                let next_value := value + (byte mod 128) * (128 ^ group) in
                if byte <? 128 then
                  if (0 <? group) && (byte =? 0) then None
                  else Some (next_value, S offset)
                else varint_groups_nat_spec rest bytes region_end (S offset)
                  (S group) next_value
            else None
        end
      else None
  end.

Theorem binary_varint_groups_exact : forall fuel bytes region_end offset group value,
  option_map (fun result => (N.to_nat (fst result), snd result))
    (varint_groups fuel bytes region_end offset group value) =
  varint_groups_nat_spec fuel bytes region_end offset group (N.to_nat value).
Proof.
  induction fuel as [|rest IH]; intros bytes region_end offset group value;
    cbn [varint_groups varint_groups_nat_spec]; try reflexivity.
  destruct (offset <? region_end); [|reflexivity].
  destruct (nth_error bytes offset) as [byte|]; [|reflexivity].
  destruct (byte_ok byte); [|reflexivity].
  destruct ((group =? 9) && (1 <? byte)); [reflexivity|].
  destruct (byte <? 128).
  - destruct ((0 <? group) && (byte =? 0)); [reflexivity|].
    cbn [option_map fst snd].
    now rewrite binary_varint_accumulator_step_exact.
  - rewrite IH.
    now rewrite binary_varint_accumulator_step_exact.
Qed.

Theorem canonical_varint_binary_exact : forall bytes region_end offset,
  option_map (fun result => (N.to_nat (fst result), snd result))
    (canonical_varint bytes region_end offset) =
  varint_groups_nat_spec 10 bytes region_end offset 0 0.
Proof. intros. apply binary_varint_groups_exact. Qed.

Theorem binary_key_number_and_wire_exact : forall key,
  N.to_nat (key / 8)%N = N.to_nat key / 8 /\
  N.to_nat (key mod 8)%N = N.to_nat key mod 8.
Proof.
  intro key. split.
  - rewrite N2Nat.inj_div. reflexivity.
  - rewrite N2Nat.inj_mod. reflexivity.
Qed.

Lemma binary_payload_length_guard_exact : forall body_start region_end payload_length,
  body_start <= region_end ->
  N.leb payload_length (N.of_nat (region_end - body_start)) =
    Nat.leb (body_start + N.to_nat payload_length) region_end.
Proof.
  intros body_start region_end payload_length bounded.
  rewrite binary_and_nat_bounds_agree, Nat2N.id.
  destruct (N.to_nat payload_length <=? region_end - body_start)
    eqn:binary;
    destruct (body_start + N.to_nat payload_length <=? region_end)
      eqn:unary;
    try reflexivity.
  - apply Nat.leb_le in binary. apply Nat.leb_gt in unary. lia.
  - apply Nat.leb_gt in binary. apply Nat.leb_le in unary. lia.
Qed.

Definition framing_field (bytes : list nat) (region_end offset last : nat)
    (numeric_order : bool) : option field_span :=
  match canonical_varint bytes region_end offset with
  | None => None
  | Some (key, key_end) =>
      let binary_number := (key / 8)%N in
      if (0 <? binary_number)%N &&
          field_number_within_limit binary_number then
        let number := N.to_nat binary_number in
        if negb numeric_order || (last <=? number) then
        match N.to_nat (key mod 8)%N with
        | 0 =>
            match canonical_varint bytes region_end key_end with
            | Some (value, value_end) =>
                Some (FieldSpan number WireVarint offset key_end key_end
                  value_end (Some value))
            | None => None
            end
        | 1 =>
            let payload_end := key_end + 8 in
            if payload_end <=? region_end then
              Some (FieldSpan number WireFixed64 offset key_end key_end
                payload_end None)
            else None
        | 2 =>
            match canonical_varint bytes region_end key_end with
            | Some (payload_length, body_start) =>
                if N.leb payload_length
                    (N.of_nat (region_end - body_start)) then
                  let body_end := body_start + N.to_nat payload_length in
                  Some (FieldSpan number WireLength offset key_end body_start
                    body_end None)
                else None
            | None => None
            end
        | 5 =>
            let payload_end := key_end + 4 in
            if payload_end <=? region_end then
              Some (FieldSpan number WireFixed32 offset key_end key_end
                payload_end None)
            else None
        | _ => None
        end
        else None
      else None
  end.

(** The guard is the cursor's explicit progress/range contract. It is
    redundant for a successfully decoded key, but keeping it executable
    makes the finite bound independent of a trusted progress assumption. *)
Definition checked_field (bytes : list nat) (region_end offset last : nat)
    (numeric_order : bool) : option field_span :=
  match framing_field bytes region_end offset last numeric_order with
  | None => None
  | Some field =>
      if (span_start field =? offset) && (offset <? span_end field) &&
         (span_end field <=? region_end) then Some field else None
  end.

Inductive scan_result :=
| Parsed (fields : list field_span)
| Rejected
| BudgetExhausted.

(** The reference is a direct recursive specification of the remaining
    suffix. It never constructs or reads the explicit cursor state below. *)
Fixpoint reference_remaining (fuel : nat) (bytes : list nat)
    (region_end offset last : nat) (numeric_order : bool) : scan_result :=
  if offset =? region_end then Parsed []
  else
    match checked_field bytes region_end offset last numeric_order with
    | None => Rejected
    | Some field =>
        match fuel with
        | 0 => BudgetExhausted
        | S rest =>
            match reference_remaining rest bytes region_end
                (span_end field) (span_number field) numeric_order with
            | Parsed suffix => Parsed (field :: suffix)
            | Rejected => Rejected
            | BudgetExhausted => BudgetExhausted
            end
        end
    end.

Record cursor_state := CursorState {
  cursor_at : nat;
  cursor_last : nat;
  cursor_reversed : list field_span
}.

Inductive cursor_action :=
| CursorEof
| CursorReject
| CursorAdvance (next : cursor_state).

Definition cursor_step (bytes : list nat) (region_end : nat)
    (numeric_order : bool) (state : cursor_state) : cursor_action :=
  if cursor_at state =? region_end then CursorEof
  else
    match checked_field bytes region_end (cursor_at state)
        (cursor_last state) numeric_order with
    | None => CursorReject
    | Some field => CursorAdvance (CursorState (span_end field)
        (span_number field) (field :: cursor_reversed state))
    end.

Fixpoint cursor_worklist (fuel : nat) (bytes : list nat)
    (region_end : nat) (numeric_order : bool)
    (state : cursor_state) : scan_result :=
  match cursor_step bytes region_end numeric_order state with
  | CursorEof => Parsed (rev (cursor_reversed state))
  | CursorReject => Rejected
  | CursorAdvance next =>
      match fuel with
      | 0 => BudgetExhausted
      | S rest => cursor_worklist rest bytes region_end numeric_order next
      end
  end.

Definition map_parsed (f : list field_span -> list field_span)
    (result : scan_result) : scan_result :=
  match result with
  | Parsed fields => Parsed (f fields)
  | Rejected => Rejected
  | BudgetExhausted => BudgetExhausted
  end.

Theorem worklist_equals_recursive_reference_with_accumulator :
  forall fuel bytes region_end numeric_order offset last accumulator,
    cursor_worklist fuel bytes region_end numeric_order
      (CursorState offset last accumulator) =
    map_parsed (fun suffix => rev accumulator ++ suffix)
      (reference_remaining fuel bytes region_end offset last numeric_order).
Proof.
  induction fuel as [|fuel IH]; intros bytes region_end numeric_order offset last acc;
    cbn [cursor_worklist reference_remaining cursor_step cursor_at cursor_last
         cursor_reversed]; unfold cursor_step;
    cbn [cursor_at cursor_last cursor_reversed].
  - destruct (offset =? region_end); simpl; [now rewrite app_nil_r|].
    destruct (checked_field bytes region_end offset last numeric_order);
      reflexivity.
  - destruct (offset =? region_end); simpl; [now rewrite app_nil_r|].
    destruct (checked_field bytes region_end offset last numeric_order)
      as [field|] eqn:field_result; [|reflexivity].
    rewrite (IH bytes region_end numeric_order (span_end field)
      (span_number field) (field :: acc)).
    destruct (reference_remaining fuel bytes region_end (span_end field)
      (span_number field) numeric_order) as [suffix| |]; simpl;
      try reflexivity.
    simpl. now rewrite <- app_assoc.
Qed.

Definition reference_region (bytes : list nat) (region_start region_end : nat)
    (numeric_order : bool) : scan_result :=
  if region_ok bytes region_start region_end then
    reference_remaining (region_end - region_start) bytes region_end
      region_start 0 numeric_order
  else Rejected.

Definition worklist_region (bytes : list nat) (region_start region_end : nat)
    (numeric_order : bool) : scan_result :=
  if region_ok bytes region_start region_end then
    cursor_worklist (region_end - region_start) bytes region_end numeric_order
      (CursorState region_start 0 [])
  else Rejected.

Theorem worklist_and_reference_agree_on_every_finite_region :
  forall bytes region_start region_end numeric_order,
    worklist_region bytes region_start region_end numeric_order =
    reference_region bytes region_start region_end numeric_order.
Proof.
  intros bytes region_start region_end numeric_order.
  unfold worklist_region, reference_region.
  destruct (region_ok bytes region_start region_end); [|reflexivity].
  rewrite worklist_equals_recursive_reference_with_accumulator.
  destruct (reference_remaining (region_end - region_start) bytes
    region_end region_start 0 numeric_order); reflexivity.
Qed.

Lemma checked_field_has_strict_bounded_progress :
  forall bytes region_end offset last ordered field,
    checked_field bytes region_end offset last ordered = Some field ->
    span_start field = offset /\ offset < span_end field /\
    span_end field <= region_end.
Proof.
  intros bytes region_end offset last ordered field result.
  unfold checked_field in result.
  destruct (framing_field bytes region_end offset last ordered)
    as [framed|] eqn:framing; [|discriminate].
  destruct ((span_start framed =? offset) && (offset <? span_end framed) &&
    (span_end framed <=? region_end)) eqn:checks; [|discriminate].
  inversion result; subst field.
  repeat rewrite andb_true_iff in checks.
  destruct checks as [[start progress] bounded].
  apply Nat.eqb_eq in start.
  apply Nat.ltb_lt in progress.
  apply Nat.leb_le in bounded.
  repeat split; assumption.
Qed.

Theorem cursor_step_has_strict_bounded_progress :
  forall bytes region_end ordered state next,
    cursor_step bytes region_end ordered state = CursorAdvance next ->
    cursor_at state < cursor_at next /\ cursor_at next <= region_end.
Proof.
  intros bytes region_end ordered [offset last acc] next step.
  unfold cursor_step in step; simpl in step.
  destruct (offset =? region_end) eqn:eof; [discriminate|].
  destruct (checked_field bytes region_end offset last ordered)
    as [field|] eqn:parsed; [|discriminate].
  inversion step; subst next; simpl.
  pose proof (checked_field_has_strict_bounded_progress _ _ _ _ _ _ parsed)
    as [_ [progress bounded]].
  split; assumption.
Qed.

Theorem cursor_step_is_deterministic :
  forall bytes region_end ordered state left right,
    cursor_step bytes region_end ordered state = CursorAdvance left ->
    cursor_step bytes region_end ordered state = CursorAdvance right ->
    left = right.
Proof. intros; congruence. Qed.

Theorem worklist_cannot_exhaust_its_structural_byte_budget :
  forall fuel bytes region_end ordered state,
    cursor_at state <= region_end ->
    region_end - cursor_at state <= fuel ->
    cursor_worklist fuel bytes region_end ordered state <> BudgetExhausted.
Proof.
  induction fuel as [|fuel IH]; intros bytes region_end ordered state
    within budget.
  - destruct state as [offset last acc]. simpl in *.
    assert (offset = region_end) by lia.
    subst offset. unfold cursor_worklist, cursor_step.
    now rewrite Nat.eqb_refl.
  - cbn [cursor_worklist].
    destruct (cursor_step bytes region_end ordered state)
      as [| |next] eqn:action; try discriminate.
    pose proof (cursor_step_has_strict_bounded_progress
      _ _ _ _ _ action) as [progress bounded].
    apply (IH bytes region_end ordered next); lia.
Qed.

Theorem finite_region_scan_terminates_without_budget_exhaustion :
  forall bytes region_start region_end ordered,
    worklist_region bytes region_start region_end ordered <> BudgetExhausted /\
    reference_region bytes region_start region_end ordered <> BudgetExhausted.
Proof.
  intros bytes region_start region_end ordered.
  destruct (region_ok bytes region_start region_end) eqn:ok.
  - pose proof ok as ok_components.
    unfold region_ok in ok_components.
    apply andb_true_iff in ok_components as [bounds _].
    apply andb_true_iff in bounds as [start_end end_bytes].
    apply Nat.leb_le in start_end.
    assert (finite :
      cursor_worklist (region_end - region_start) bytes region_end ordered
        (CursorState region_start 0 []) <> BudgetExhausted).
    { apply worklist_cannot_exhaust_its_structural_byte_budget;
        simpl; lia. }
    split.
    + unfold worklist_region. rewrite ok. exact finite.
    + rewrite <- worklist_and_reference_agree_on_every_finite_region.
      unfold worklist_region. rewrite ok. exact finite.
  - unfold worklist_region, reference_region.
    rewrite ok. split; discriminate.
Qed.

(** Finite controls pin exact half-open offsets and malformed refusal.
    Numeric-order mode matches CanonicalFieldCursor::new; slot-order mode
    matches in_prost_slot_order, leaving slot checks to the typed consumer. *)
Example empty_region_is_accepted :
  worklist_region [] 0 0 true = Parsed [].
Proof. vm_compute. reflexivity. Qed.

Example canonical_varint_and_length_fields_have_exact_spans :
  worklist_region [8; 150; 1; 18; 2; 65; 66] 0 7 true =
    Parsed [FieldSpan 1 WireVarint 0 1 1 3 (Some 150%N);
      FieldSpan 2 WireLength 3 4 5 7 None].
Proof. vm_compute. reflexivity. Qed.

Example fixed_widths_have_exact_spans :
  worklist_region [9; 1; 2; 3; 4; 5; 6; 7; 8;
                   21; 9; 10; 11; 12] 0 14 true =
    Parsed [FieldSpan 1 WireFixed64 0 1 1 9 None;
      FieldSpan 2 WireFixed32 9 10 10 14 None].
Proof. vm_compute. reflexivity. Qed.

Example bounded_subregion_preserves_absolute_offsets :
  worklist_region [255; 8; 1; 255] 1 3 true =
    Parsed [FieldSpan 1 WireVarint 1 2 2 3 (Some 1%N)].
Proof. vm_compute. reflexivity. Qed.

Example truncated_or_nonminimal_varints_refuse :
  worklist_region [128] 0 1 true = Rejected /\
  worklist_region [8; 128; 0] 0 3 true = Rejected /\
  worklist_region [128; 0; 1] 0 3 true = Rejected.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example oversized_varint_and_bad_tags_refuse :
  worklist_region [255;255;255;255;255;255;255;255;255;2] 0 10 true = Rejected /\
  worklist_region [0] 0 1 true = Rejected /\
  worklist_region [11] 0 1 true = Rejected.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example terminal_group_nine_preserves_max_u64 :
  canonical_varint [255;255;255;255;255;255;255;255;255;1] 10 0 =
    Some (18446744073709551615%N, 10).
Proof. vm_compute. reflexivity. Qed.

Example truncated_fixed_and_length_fields_refuse :
  worklist_region [9; 1; 2; 3] 0 4 true = Rejected /\
  worklist_region [18; 3; 1; 2] 0 4 true = Rejected.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example numeric_order_is_optional_but_progress_is_not :
  worklist_region [16; 0; 8; 0] 0 4 true = Rejected /\
  worklist_region [16; 0; 8; 0] 0 4 false =
    Parsed [FieldSpan 2 WireVarint 0 1 1 2 (Some 0%N);
      FieldSpan 1 WireVarint 2 3 3 4 (Some 0%N)].
Proof. vm_compute. repeat split; reflexivity. Qed.

Print Assumptions worklist_and_reference_agree_on_every_finite_region.
Print Assumptions checked_field_has_strict_bounded_progress.
Print Assumptions finite_region_scan_terminates_without_budget_exhaustion.
Print Assumptions canonical_varint_and_length_fields_have_exact_spans.
Print Assumptions truncated_or_nonminimal_varints_refuse.
