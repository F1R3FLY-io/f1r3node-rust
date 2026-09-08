From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import EPathMap EPM1.

Import ListNotations.
Set Implicit Arguments.

Module SingletonPrefilter.

Definition payload_route_hint (storage : mode) (payload : bytes)
    : option (mode * nat) :=
  match decode_frame payload with
  | Some (_, after_arena) =>
      match decode_varint after_arena with
      | Some (count, framed_values) =>
          if count <=? length framed_values then Some (storage, count) else None
      | None => None
      end
  | None => None
  end.

Definition snapshot_route_hint (input : bytes) : option (mode * nat) :=
  match input with
  | 69 :: 80 :: 77 :: 49 :: 1 :: storage :: payload =>
      match storage with
      | 0 => payload_route_hint Neutral payload
      | 1 => payload_route_hint SetMode payload
      | 2 => payload_route_hint MapMode payload
      | _ => None
      end
  | _ => None
  end.

Definition singleton_hint (input : bytes) : bool :=
  match snapshot_route_hint input with
  | Some (MapMode, 1) => true
  | _ => false
  end.

Lemma decoded_varint_consumes_input :
  forall input value rest,
    decode_varint input = Some (value, rest) -> length rest < length input.
Proof.
  induction input as [|head tail induction]; intros value rest parsed.
  - rewrite decode_varint_nil in parsed. discriminate.
  - rewrite decode_varint_cons in parsed.
    destruct (head <? 128) eqn:small.
    + inversion parsed; subst. simpl. lia.
    + destruct (head <? 256); try discriminate.
      destruct (decode_varint tail) as [[quotient suffix]|] eqn:child;
        try discriminate.
      destruct (quotient =? 0); try discriminate.
      inversion parsed; subst.
      specialize (induction _ _ eq_refl). simpl. lia.
Qed.

Lemma decoded_frame_consumes_input :
  forall input value rest,
    decode_frame input = Some (value, rest) -> length rest < length input.
Proof.
  intros input value rest parsed. unfold decode_frame in parsed.
  destruct (decode_varint input) as [[size suffix]|] eqn:header;
    try discriminate.
  destruct (size <=? length suffix); try discriminate.
  inversion parsed; subst.
  pose proof (@decoded_varint_consumes_input input size suffix header).
  rewrite length_skipn. lia.
Qed.

Lemma decoded_frames_count_and_bound :
  forall count input values rest,
    decode_frames count input = Some (values, rest) ->
    length values = count /\ count <= length input.
Proof.
  induction count as [|count induction]; intros input values rest parsed; simpl in parsed.
  - inversion parsed; subst. simpl. lia.
  - destruct (decode_frame input) as [[value suffix]|] eqn:frame;
      try discriminate.
    destruct (decode_frames count suffix) as [[children tail]|] eqn:children_parsed;
      try discriminate.
    inversion parsed; subst.
    pose proof (induction _ _ _ children_parsed) as [exact_count bounded].
    pose proof (@decoded_frame_consumes_input input value suffix frame).
    simpl. lia.
Qed.

Theorem decoded_epm1_prefix_determines_route_hint :
  forall input snapshot rest,
    decode_epm1_prefix input = Some (snapshot, rest) ->
    snapshot_route_hint input =
      Some (snapshot_mode snapshot, length (value_bodies snapshot)).
Proof.
  intros input snapshot rest parsed.
  unfold decode_epm1_prefix in parsed.
  unfold snapshot_route_hint.
  repeat match goal with
  | parsed : context [match ?subject with _ => _ end] |- _ =>
      destruct subject eqn:?;
      cbn -[decode_frame decode_varint decode_frames payload_route_hint] in parsed |- *;
      try discriminate
  end.
  all: inversion parsed; subst; unfold payload_route_hint.
  all: repeat match goal with
  | known : ?expression = Some _ |- context [?expression] => rewrite known
  end.
  all: match goal with
  | known : decode_frames _ _ = Some _ |- _ =>
      pose proof (@decoded_frames_count_and_bound _ _ _ _ known) as [exact_count bounded]
  end.
  all: apply Nat.leb_le in bounded.
  all: rewrite bounded; cbn; now rewrite exact_count.
Qed.

Record inspection : Type := Inspection {
  inspected_snapshot : epm1_snapshot;
  inspected_keys : list bytes;
  inspected_output : bytes
}.

Definition inspector_refines_epm1_framing (inspect : bytes -> option inspection) : Prop :=
  forall input result,
    inspect input = Some result ->
    decode_epm1_prefix input = Some (inspected_snapshot result, []).

Definition singleton_inspection (result : inspection) : bool :=
  match snapshot_mode (inspected_snapshot result) with
  | MapMode =>
      (length (inspected_keys result) =? 1) &&
      (length (value_bodies (inspected_snapshot result)) =? 1)
  | _ => false
  end.

Definition original_singleton_import (inspect : bytes -> option inspection)
    (input : bytes) : option inspection :=
  match inspect input with
  | Some result => if singleton_inspection result then Some result else None
  | None => None
  end.

Definition prefiltered_singleton_import (inspect : bytes -> option inspection)
    (input : bytes) : option inspection :=
  if singleton_hint input then original_singleton_import inspect input else None.

Lemma original_success_has_singleton_hint :
  forall inspect input result,
    inspector_refines_epm1_framing inspect ->
    original_singleton_import inspect input = Some result ->
    singleton_hint input = true.
Proof.
  intros inspect input result refinement imported.
  unfold original_singleton_import in imported.
  destruct (inspect input) as [observed|] eqn:inspected; try discriminate.
  destruct (singleton_inspection observed) eqn:single; try discriminate.
  inversion imported; subst.
  pose proof (refinement _ _ inspected) as framed.
  pose proof (@decoded_epm1_prefix_determines_route_hint _ _ _ framed) as hinted.
  unfold singleton_inspection in single.
  destruct (snapshot_mode (inspected_snapshot result)) eqn:storage; try discriminate.
  apply andb_true_iff in single. destruct single as [_ count].
  apply Nat.eqb_eq in count.
  unfold singleton_hint. rewrite hinted, count. reflexivity.
Qed.

Theorem singleton_prefilter_preserves_result :
  forall inspect input,
    inspector_refines_epm1_framing inspect ->
    prefiltered_singleton_import inspect input = original_singleton_import inspect input.
Proof.
  intros inspect input refinement. unfold prefiltered_singleton_import.
  destruct (singleton_hint input) eqn:hint; [reflexivity|].
  destruct (original_singleton_import inspect input) as [result|] eqn:imported;
    [|reflexivity].
  pose proof (@original_success_has_singleton_hint _ _ _ refinement imported). congruence.
Qed.

Definition delegate_after_import {Observation : Type}
    (observe : inspection -> Observation) (fallback : bytes -> Observation)
    (input : bytes) (result : option inspection) : Observation :=
  match result with Some accepted => observe accepted | None => fallback input end.

Theorem singleton_prefilter_preserves_fallback_observation :
  forall (Observation : Type) inspect (observe : inspection -> Observation)
      (fallback : bytes -> Observation) input,
    inspector_refines_epm1_framing inspect ->
    delegate_after_import observe fallback input (prefiltered_singleton_import inspect input) =
    delegate_after_import observe fallback input (original_singleton_import inspect input).
Proof.
  intros. now rewrite singleton_prefilter_preserves_result.
Qed.

Inductive import_phase : Type :=
| HintPhase
| InspectPhase
| Finished : option inspection -> import_phase.

Inductive import_event : Type :=
| ReadRouteHint
| InvokeFullInspection.

Inductive import_step (inspect : bytes -> option inspection) (input : bytes)
    : import_phase -> import_event -> import_phase -> Prop :=
| HintReject : singleton_hint input = false ->
    import_step inspect input HintPhase ReadRouteHint (Finished None)
| HintContinue : singleton_hint input = true ->
    import_step inspect input HintPhase ReadRouteHint InspectPhase
| InspectFinish :
    import_step inspect input InspectPhase InvokeFullInspection
      (Finished (original_singleton_import inspect input)).

Inductive import_run (inspect : bytes -> option inspection) (input : bytes)
    : import_phase -> list import_event -> option inspection -> Prop :=
| RunFinished : forall result,
    import_run inspect input (Finished result) [] result
| RunStep : forall before event after events result,
    import_step inspect input before event after ->
    import_run inspect input after events result ->
    import_run inspect input before (event :: events) result.

Theorem prefilter_machine_has_exact_execution :
  forall inspect input,
    exists events,
      import_run inspect input HintPhase events
        (prefiltered_singleton_import inspect input).
Proof.
  intros inspect input. unfold prefiltered_singleton_import.
  destruct (singleton_hint input) eqn:hint.
  - exists [ReadRouteHint; InvokeFullInspection].
    eapply RunStep; [apply HintContinue; exact hint|].
    eapply RunStep; [apply InspectFinish|apply RunFinished].
  - exists [ReadRouteHint].
    eapply RunStep; [apply HintReject; exact hint|apply RunFinished].
Qed.

Lemma finished_run_has_no_events :
  forall inspect input initial events result,
    import_run inspect input (Finished initial) events result ->
    events = [] /\ result = initial.
Proof.
  intros inspect input initial events result run.
  inversion run as [|before event after tail returned step rest]; subst.
  - auto.
  - inversion step.
Qed.

Theorem width_rejection_never_invokes_full_inspection :
  forall inspect input events result,
    singleton_hint input = false ->
    import_run inspect input HintPhase events result ->
    events = [ReadRouteHint] /\ result = None /\
    ~ In InvokeFullInspection events.
Proof.
  intros inspect input events result rejected run.
  inversion run as [|before event after tail returned step rest]; subst.
  inversion step; subst; try congruence.
  pose proof (@finished_run_has_no_events _ _ _ _ _ rest) as [empty same].
  subst. split; [reflexivity|]. split; [reflexivity|].
  simpl. intuition discriminate.
Qed.

Theorem hinted_singleton_still_invokes_full_inspection :
  forall inspect input events result,
    singleton_hint input = true ->
    import_run inspect input HintPhase events result ->
    events = [ReadRouteHint; InvokeFullInspection] /\
    result = original_singleton_import inspect input.
Proof.
  intros inspect input events result hinted run.
  inversion run as [|before event after tail returned step rest]; subst.
  inversion step; subst; try congruence.
  inversion rest as [|before' event' after' tail' returned' step' rest']; subst.
  inversion step'; subst.
  pose proof (@finished_run_has_no_events _ _ _ _ _ rest') as [empty same].
  subst. auto.
Qed.

Definition synthetic_topology_matches (snapshot : epm1_snapshot) : bool :=
  match snapshot_mode snapshot with
  | MapMode => length (topology_arena snapshot) =? length (value_bodies snapshot)
  | SetMode => length (value_bodies snapshot) =? 0
  | Neutral =>
      (length (topology_arena snapshot) =? 0) &&
      (length (value_bodies snapshot) =? 0)
  end.

Definition synthetic_inspector (input : bytes) : option inspection :=
  match decode_epm1_prefix input with
  | Some (snapshot, []) =>
      if synthetic_topology_matches snapshot then
        Some (Inspection snapshot (map (fun marker => [marker]) (topology_arena snapshot)) input)
      else None
  | _ => None
  end.

Lemma synthetic_inspector_refines_framing :
  inspector_refines_epm1_framing synthetic_inspector.
Proof.
  intros input result inspected. unfold synthetic_inspector in inspected.
  destruct (decode_epm1_prefix input) as [[snapshot rest]|] eqn:parsed;
    try discriminate.
  destruct rest; try discriminate.
  destruct (synthetic_topology_matches snapshot); try discriminate.
  inversion inspected; subst. reflexivity.
Qed.

Definition valid_singleton : bytes := encode_epm1 (Epm1Snapshot MapMode [0] [[7]]).
Definition valid_wide : bytes := encode_epm1 (Epm1Snapshot MapMode [0; 0] [[7]; [8]]).
Definition forged_count : bytes := encode_epm1 (Epm1Snapshot MapMode [0; 0] [[7]]).
Definition wrong_mode : bytes := encode_epm1 (Epm1Snapshot SetMode [0] []).
Definition malformed_hinted_singleton : bytes :=
  [69; 80; 77; 49; 1; 2; 1; 0; 1; 2].

Theorem valid_singleton_control :
  exists result,
    original_singleton_import synthetic_inspector valid_singleton = Some result /\
    prefiltered_singleton_import synthetic_inspector valid_singleton = Some result /\
    inspected_output result = valid_singleton.
Proof. eexists. vm_compute. repeat split; reflexivity. Qed.

Theorem valid_wide_control :
  (exists result, synthetic_inspector valid_wide = Some result) /\
  singleton_hint valid_wide = false /\
  original_singleton_import synthetic_inspector valid_wide = None /\
  prefiltered_singleton_import synthetic_inspector valid_wide = None.
Proof. split; [eexists; vm_compute; reflexivity|]. vm_compute. auto. Qed.

Theorem forged_count_control :
  singleton_hint forged_count = true /\
  synthetic_inspector forged_count = None /\
  prefiltered_singleton_import synthetic_inspector forged_count = None.
Proof. vm_compute. auto. Qed.

Theorem wrong_mode_control :
  (exists result, synthetic_inspector wrong_mode = Some result) /\
  singleton_hint wrong_mode = false /\
  prefiltered_singleton_import synthetic_inspector wrong_mode = None.
Proof. split; [eexists; vm_compute; reflexivity|]. vm_compute. auto. Qed.

Theorem malformed_control :
  singleton_hint malformed_hinted_singleton = true /\
  synthetic_inspector malformed_hinted_singleton = None /\
  prefiltered_singleton_import synthetic_inspector malformed_hinted_singleton = None /\
  singleton_hint [69; 80] = false.
Proof. vm_compute. auto. Qed.

Definition hint_as_admission_mutant (input : bytes) : bool := singleton_hint input.

Theorem hint_is_not_admission_control :
  hint_as_admission_mutant forged_count = true /\
  hint_as_admission_mutant malformed_hinted_singleton = true /\
  prefiltered_singleton_import synthetic_inspector forged_count = None /\
  prefiltered_singleton_import synthetic_inspector malformed_hinted_singleton = None.
Proof. vm_compute. auto. Qed.

Print Assumptions decoded_epm1_prefix_determines_route_hint.
Print Assumptions singleton_prefilter_preserves_result.
Print Assumptions singleton_prefilter_preserves_fallback_observation.
Print Assumptions prefilter_machine_has_exact_execution.
Print Assumptions width_rejection_never_invokes_full_inspection.
Print Assumptions hinted_singleton_still_invokes_full_inspection.
Print Assumptions valid_singleton_control.
Print Assumptions valid_wide_control.
Print Assumptions forged_count_control.
Print Assumptions wrong_mode_control.
Print Assumptions malformed_control.
Print Assumptions hint_is_not_admission_control.

End SingletonPrefilter.

From Stdlib Require Import NArith ZArith.

Module BinaryVar.

Definition u32_limit : N := 4294967296%N.
Definition signed32 (value : Z) : Prop := (-2147483648 <= value < 2147483648)%Z.

Fixpoint binary_varint (input : bytes) : option (N * bytes) :=
  match input with
  | [] => None
  | head :: tail =>
      if head <? 128 then Some (N.of_nat head, tail)
      else if head <? 256 then
        match binary_varint tail with
        | Some (quotient, rest) =>
            if N.eqb quotient 0 then None
            else Some ((N.of_nat (head - 128) + 128 * quotient)%N, rest)
        | None => None
        end
      else None
  end.

Theorem binary_varint_refines_epm1 :
  forall input,
    binary_varint input =
      option_map (fun parsed => (N.of_nat (fst parsed), snd parsed)) (decode_varint input).
Proof.
  induction input as [|head tail induction].
  - rewrite decode_varint_nil. reflexivity.
  - rewrite decode_varint_cons. cbn [binary_varint].
    destruct (head <? 128); [reflexivity|].
    destruct (head <? 256); [|reflexivity].
    rewrite induction.
    destruct (decode_varint tail) as [[quotient rest]|]; [|reflexivity].
    destruct quotient; [reflexivity|].
    cbn [option_map fst snd].
    change (Some ((N.of_nat (head - 128) + 128 * N.of_nat (S quotient))%N, rest) =
      Some (N.of_nat ((head - 128) + 128 * S quotient), rest)).
    rewrite Nat2N.inj_add, Nat2N.inj_mul. reflexivity.
Qed.

Fixpoint varint_budget (fuel : nat) : N :=
  match fuel with 0 => 128%N | S remaining => (128 * varint_budget remaining)%N end.

Fixpoint emit_binary_varint (fuel : nat) (value : N) : bytes :=
  if N.ltb value 128 then [N.to_nat value]
  else match fuel with
       | 0 => []
       | S remaining =>
           (128 + N.to_nat (value mod 128)) :: emit_binary_varint remaining (value / 128)
       end.

Lemma emit_binary_varint_step :
  forall fuel value,
    emit_binary_varint (S fuel) value =
      if N.ltb value 128 then [N.to_nat value]
      else (128 + N.to_nat (value mod 128)) :: emit_binary_varint fuel (value / 128).
Proof. reflexivity. Qed.

Lemma radix_decomposition :
  forall value,
    (value = 128 * (value / 128) + value mod 128)%N /\
    (value mod 128 < 128)%N.
Proof.
  intro value. split.
  - apply N.div_mod. discriminate.
  - apply N.mod_lt. discriminate.
Qed.

Lemma binary_varint_emitted_round_trip :
  forall fuel value suffix,
    (value < varint_budget fuel)%N ->
    binary_varint (emit_binary_varint fuel value ++ suffix) = Some (value, suffix).
Proof.
  induction fuel as [|fuel induction]; intros value suffix bounded.
  - cbn [varint_budget] in bounded.
    assert (small : N.ltb value 128 = true) by (apply N.ltb_lt; exact bounded).
    cbn [emit_binary_varint]. rewrite small. cbn [app binary_varint].
    assert (byte_small : (N.to_nat value <? 128) = true) by (apply Nat.ltb_lt; lia).
    rewrite byte_small, N2Nat.id. reflexivity.
  - cbn [emit_binary_varint]. destruct (N.ltb value 128) eqn:small.
    + cbn [app binary_varint]. apply N.ltb_lt in small.
      assert (byte_small : (N.to_nat value <? 128) = true) by (apply Nat.ltb_lt; lia).
      rewrite byte_small, N2Nat.id. reflexivity.
    + apply N.ltb_ge in small.
      pose proof (radix_decomposition value) as [decomposition remainder_bound].
      assert (quotient_positive : (0 < value / 128)%N) by nia.
      assert (quotient_bounded : (value / 128 < varint_budget fuel)%N).
      { apply N.div_lt_upper_bound; [discriminate|exact bounded]. }
      assert (byte_not_small : (128 + N.to_nat (value mod 128) <? 128) = false)
        by (apply Nat.ltb_ge; apply Nat.le_add_r).
      assert (byte_bounded : (128 + N.to_nat (value mod 128) <? 256) = true)
        by (apply Nat.ltb_lt; clear - remainder_bound; lia).
      cbn [app binary_varint]. rewrite byte_not_small, byte_bounded.
      rewrite induction by exact quotient_bounded.
      assert (nonzero : N.eqb (value / 128) 0 = false) by (apply N.eqb_neq; lia).
      rewrite nonzero.
      replace (128 + N.to_nat (value mod 128) - 128) with (N.to_nat (value mod 128))
        by (symmetry; apply Nat.add_sub_eq_l; reflexivity).
      rewrite N2Nat.id. f_equal. f_equal. clear - decomposition. nia.
Qed.

Lemma binary_varint_exact_reconstruction :
  forall fuel input value rest,
    binary_varint input = Some (value, rest) ->
    (value < varint_budget fuel)%N ->
    input = emit_binary_varint fuel value ++ rest.
Proof.
  induction fuel as [|fuel induction]; intros input value rest parsed bounded;
    destruct input as [|head tail]; try discriminate;
    cbn [binary_varint] in parsed;
    destruct (head <? 128) eqn:small.
  - inversion parsed; subst. apply Nat.ltb_lt in small.
    cbn [emit_binary_varint].
    assert (small_binary : N.ltb (N.of_nat head) 128 = true) by (apply N.ltb_lt; lia).
    rewrite small_binary, Nat2N.id. reflexivity.
  - destruct (head <? 256) eqn:byte_bound; try discriminate.
    destruct (binary_varint tail) as [[quotient suffix]|] eqn:child; try discriminate.
    destruct (N.eqb quotient 0) eqn:nonzero; try discriminate.
    inversion parsed; subst. apply N.eqb_neq in nonzero.
    change (N.of_nat (head - 128) + 128 * quotient < 128)%N in bounded.
    exfalso. clear - bounded nonzero. nia.
  - inversion parsed; subst. apply Nat.ltb_lt in small.
    cbn [emit_binary_varint].
    assert (small_binary : N.ltb (N.of_nat head) 128 = true) by (apply N.ltb_lt; lia).
    rewrite small_binary, Nat2N.id. reflexivity.
  - destruct (head <? 256) eqn:byte_bound; try discriminate.
    destruct (binary_varint tail) as [[quotient suffix]|] eqn:child; try discriminate.
    destruct (N.eqb quotient 0) eqn:nonzero; try discriminate.
    inversion parsed; subst.
    apply Nat.ltb_ge in small. apply Nat.ltb_lt in byte_bound.
    apply N.eqb_neq in nonzero.
    assert (low_bound : (N.of_nat (head - 128) < 128)%N)
      by (clear - small byte_bound; lia).
    assert (quotient_bounded : (quotient < varint_budget fuel)%N).
    { change (N.of_nat (head - 128) + 128 * quotient < 128 * varint_budget fuel)%N in bounded.
      clear - bounded. nia. }
    specialize (induction _ _ _ child quotient_bounded).
    change (head :: tail =
      emit_binary_varint (S fuel) (N.of_nat (head - 128) + 128 * quotient)%N ++ rest).
    assert (large : N.ltb (N.of_nat (head - 128) + 128 * quotient) 128 = false)
      by (apply N.ltb_ge; clear - nonzero; nia).
    rewrite emit_binary_varint_step, large.
    assert (exact_div : ((N.of_nat (head - 128) + 128 * quotient) / 128 = quotient)%N).
    { symmetry. apply N.div_unique with (r := N.of_nat (head - 128));
        [exact low_bound|apply N.add_comm]. }
    assert (exact_mod : ((N.of_nat (head - 128) + 128 * quotient) mod 128 =
                         N.of_nat (head - 128))%N).
    { symmetry. apply N.mod_unique with (q := quotient);
        [exact low_bound|apply N.add_comm]. }
    rewrite exact_div, exact_mod, Nat2N.id.
    replace (128 + (head - 128)) with head by (clear - small; lia).
    cbn [app]. now rewrite <- induction.
Qed.

Lemma emit_binary_varint_length :
  forall fuel value, length (emit_binary_varint fuel value) <= S fuel.
Proof.
  induction fuel as [|fuel induction]; intro value; cbn [emit_binary_varint];
    destruct (N.ltb value 128); cbn; try lia.
  specialize (induction (value / 128)%N). lia.
Qed.

Definition parse_u32 (input : bytes) : option N :=
  match binary_varint input with
  | Some (value, []) => if N.ltb value u32_limit then Some value else None
  | _ => None
  end.

Theorem canonical_u32_exact_image :
  forall input value,
    parse_u32 input = Some value <->
    (value < u32_limit)%N /\ input = emit_binary_varint 4 value.
Proof.
  intros input value. split.
  - unfold parse_u32. destruct (binary_varint input) as [[observed rest]|] eqn:parsed;
      try discriminate.
    destruct rest; try discriminate.
    destruct (N.ltb observed u32_limit) eqn:bounded; try discriminate.
    intro same. inversion same; subst. apply N.ltb_lt in bounded.
    split; [exact bounded|].
    pose proof (@binary_varint_exact_reconstruction 4 input value [] parsed) as exact_bytes.
    rewrite app_nil_r in exact_bytes. apply exact_bytes.
    change (value < 34359738368)%N. unfold u32_limit in bounded. lia.
  - intros [bounded ->]. unfold parse_u32.
    pose proof (@binary_varint_emitted_round_trip 4 value []) as parsed.
    rewrite app_nil_r in parsed. rewrite parsed.
    + apply N.ltb_lt in bounded. now rewrite bounded.
    + change (value < 34359738368)%N. unfold u32_limit in bounded. lia.
Qed.

Lemma accepted_u32_has_at_most_five_bytes :
  forall input value, parse_u32 input = Some value -> length input <= 5.
Proof.
  intros input value parsed. apply canonical_u32_exact_image in parsed.
  destruct parsed as [_ ->]. apply emit_binary_varint_length.
Qed.

Definition zigzag (value : Z) : N :=
  if Z.ltb value 0 then Z.to_N (-2 * value - 1) else Z.to_N (2 * value).

Definition unzigzag (value : N) : Z :=
  let whole := Z.of_N value in
  if Z.eqb (whole mod 2) 0 then whole / 2 else -(whole / 2) - 1.

Lemma halves (value : N) :
  let whole := Z.of_N value in
  (0 <= whole / 2 /\ 0 <= whole mod 2 < 2 /\
   whole = 2 * (whole / 2) + whole mod 2)%Z.
Proof.
  cbv zeta. split.
  - apply Z.div_pos; [apply N2Z.is_nonneg|lia].
  - split.
    + apply Z.mod_pos_bound. lia.
    + apply Z.div_mod. lia.
Qed.

Lemma unzigzag_zigzag : forall value, unzigzag (zigzag value) = value.
Proof.
  intro value. unfold zigzag, unzigzag.
  destruct (Z.ltb value 0) eqn:negative.
  - apply Z.ltb_lt in negative. rewrite Z2N.id by lia.
    assert (quotient : ((-2 * value - 1) / 2 = -value - 1)%Z).
    { symmetry. apply Z.div_unique_pos with (r := 1%Z); lia. }
    assert (remainder : ((-2 * value - 1) mod 2 = 1)%Z).
    { symmetry. apply Z.mod_unique with (q := (-value - 1)%Z); lia. }
    rewrite quotient, remainder. cbn. lia.
  - apply Z.ltb_ge in negative. rewrite Z2N.id by lia.
    assert (quotient : ((2 * value) / 2 = value)%Z).
    { symmetry. apply Z.div_unique_pos with (r := 0%Z); lia. }
    assert (remainder : ((2 * value) mod 2 = 0)%Z).
    { symmetry. apply Z.mod_unique with (q := value); lia. }
    rewrite quotient, remainder. reflexivity.
Qed.

Lemma zigzag_unzigzag : forall value, zigzag (unzigzag value) = value.
Proof.
  intro value. pose proof (halves value) as [quotient_nonnegative [remainder_bound equation]].
  unfold unzigzag, zigzag.
  destruct (Z.eqb (Z.of_N value mod 2) 0) eqn:even.
  - apply Z.eqb_eq in even.
    assert (negative : Z.ltb (Z.of_N value / 2) 0 = false) by (apply Z.ltb_ge; lia).
    rewrite negative. apply N2Z.inj. rewrite Z2N.id by lia. lia.
  - apply Z.eqb_neq in even.
    assert (negative : Z.ltb (- (Z.of_N value / 2) - 1) 0 = true)
      by (apply Z.ltb_lt; lia).
    rewrite negative. apply N2Z.inj. rewrite Z2N.id by lia. lia.
Qed.

Lemma zigzag_signed32_bound :
  forall value, signed32 value -> (zigzag value < u32_limit)%N.
Proof.
  intros value bounded. unfold signed32 in bounded.
  unfold zigzag. destruct (Z.ltb value 0) eqn:negative; apply N2Z.inj_lt.
  - change (Z.of_N (Z.to_N (-2 * value - 1)) < 4294967296)%Z.
    apply Z.ltb_lt in negative. rewrite Z2N.id by lia. lia.
  - change (Z.of_N (Z.to_N (2 * value)) < 4294967296)%Z.
    apply Z.ltb_ge in negative. rewrite Z2N.id by lia. lia.
Qed.

Lemma unzigzag_u32_bound :
  forall value, (value < u32_limit)%N -> signed32 (unzigzag value).
Proof.
  intros value bounded. apply N2Z.inj_lt in bounded.
  unfold u32_limit in bounded. change (Z.of_N value < 4294967296)%Z in bounded.
  pose proof (halves value) as [quotient_nonnegative [remainder_bound equation]].
  unfold signed32, unzigzag.
  destruct (Z.eqb (Z.of_N value mod 2) 0); lia.
Qed.

Theorem signed32_zigzag_bijection :
  (forall value, signed32 value ->
      (zigzag value < u32_limit)%N /\ unzigzag (zigzag value) = value) /\
  (forall value, (value < u32_limit)%N ->
      signed32 (unzigzag value) /\ zigzag (unzigzag value) = value).
Proof.
  split; intros; split; auto using zigzag_signed32_bound, unzigzag_zigzag,
    unzigzag_u32_bound, zigzag_unzigzag.
Qed.

Inductive canonical_var : Type :=
| Absent
| Bound : Z -> canonical_var
| Free : Z -> canonical_var
| Wildcard.

Definition valid_var (value : canonical_var) : Prop :=
  match value with Bound scalar | Free scalar => signed32 scalar | _ => True end.

Definition encode_var (value : canonical_var) : bytes :=
  match value with
  | Absent => []
  | Bound scalar => 8 :: emit_binary_varint 4 (zigzag scalar)
  | Free scalar => 16 :: emit_binary_varint 4 (zigzag scalar)
  | Wildcard => [26; 0]
  end.

Definition parse_var (input : bytes) : option canonical_var :=
  match input with
  | [] => Some Absent
  | 8 :: payload => option_map (fun value => Bound (unzigzag value)) (parse_u32 payload)
  | 16 :: payload => option_map (fun value => Free (unzigzag value)) (parse_u32 payload)
  | [26; 0] => Some Wildcard
  | _ => None
  end.

Lemma valid_var_encoder_round_trip :
  forall value, valid_var value -> parse_var (encode_var value) = Some value.
Proof.
  intros value valid. destruct value; try reflexivity;
    cbn [valid_var] in valid; cbn [encode_var parse_var];
    assert (parsed : parse_u32 (emit_binary_varint 4 (zigzag z)) = Some (zigzag z)).
  all: try (apply canonical_u32_exact_image; split; [now apply zigzag_signed32_bound|reflexivity]).
  all: rewrite parsed; cbn [option_map]; now rewrite unzigzag_zigzag.
Qed.

Theorem canonical_var_exact_encoder_image :
  forall input value,
    parse_var input = Some value <-> valid_var value /\ input = encode_var value.
Proof.
  intros input value. split.
  - unfold parse_var, option_map.
    repeat match goal with
    | |- context [match ?subject with _ => _ end] =>
        destruct subject eqn:?;
        cbn -[parse_u32 unzigzag encode_var valid_var] in *;
        try discriminate
    end.
    all: intro parsed; inversion parsed; subst; cbn [valid_var encode_var]; try tauto.
    all: match goal with
    | parsed : parse_u32 _ = Some _ |- _ =>
        apply canonical_u32_exact_image in parsed; destruct parsed as [bounded exact_bytes]
    end.
    all: split; [now apply unzigzag_u32_bound|].
    all: rewrite zigzag_unzigzag, <- exact_bytes; reflexivity.
  - intros [valid ->]. now apply valid_var_encoder_round_trip.
Qed.

Theorem canonical_var_positive_controls :
  parse_var [] = Some Absent /\
  parse_var [8; 0] = Some (Bound 0) /\
  parse_var [16; 0] = Some (Free 0) /\
  parse_var [8; 255; 255; 255; 255; 15] = Some (Bound (-2147483648)) /\
  parse_var [16; 254; 255; 255; 255; 15] = Some (Free 2147483647) /\
  parse_var [16; 1] = Some (Free (-1)) /\
  parse_var [26; 0] = Some Wildcard.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem canonical_var_scalar_refusal_controls :
  parse_var [8] = None /\
  parse_var [8; 128; 0] = None /\
  parse_var [136; 0; 0] = None /\
  parse_var [8; 0; 16; 0] = None /\
  parse_var [32; 0] = None /\
  parse_var [8; 128; 128; 128; 128; 16] = None /\
  parse_var [8; 255; 255; 255; 255; 255; 1] = None.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem canonical_var_presence_and_wildcard_controls :
  encode_var Absent <> encode_var (Bound 0) /\
  encode_var Absent <> encode_var (Free 0) /\
  parse_var [26; 1] = None /\
  parse_var [26; 1; 0] = None /\
  parse_var [26; 0; 32; 0] = None /\
  parse_var [26; 128; 0] = None.
Proof. vm_compute. intuition discriminate. Qed.

Print Assumptions binary_varint_refines_epm1.
Print Assumptions canonical_u32_exact_image.
Print Assumptions signed32_zigzag_bijection.
Print Assumptions canonical_var_exact_encoder_image.
Print Assumptions canonical_var_positive_controls.
Print Assumptions canonical_var_scalar_refusal_controls.
Print Assumptions canonical_var_presence_and_wildcard_controls.

End BinaryVar.

Module Envelope.

Lemma encode_varint_fuel_step :
  forall fuel value,
    encode_varint_fuel (S fuel) value =
      if value <? 128 then [value]
      else (128 + value mod 128) :: encode_varint_fuel fuel (value / 128).
Proof. reflexivity. Qed.

Lemma decoded_varint_reconstructs_fuel :
  forall fuel input value rest,
    value < fuel -> decode_varint input = Some (value, rest) ->
    input = encode_varint_fuel fuel value ++ rest.
Proof.
  induction fuel as [|fuel induction]; intros input value rest bounded parsed; [lia|].
  destruct input as [|head tail]; [rewrite decode_varint_nil in parsed; discriminate|].
  rewrite decode_varint_cons in parsed.
  destruct (head <? 128) eqn:small.
  - inversion parsed; subst. now rewrite encode_varint_fuel_step, small.
  - destruct (head <? 256) eqn:byte_bound; try discriminate.
    destruct (decode_varint tail) as [[quotient suffix]|] eqn:child; try discriminate.
    destruct (quotient =? 0) eqn:nonzero; try discriminate.
    inversion parsed; subst.
    apply Nat.ltb_ge in small. apply Nat.ltb_lt in byte_bound.
    apply Nat.eqb_neq in nonzero.
    assert (quotient_bounded : quotient < fuel) by nia.
    specialize (induction _ _ _ quotient_bounded child).
    assert (large : ((head - 128 + 128 * quotient) <? 128) = false)
      by (apply Nat.ltb_ge; nia).
    assert (low_bound : head - 128 < 128) by lia.
    assert (exact_div : (head - 128 + 128 * quotient) / 128 = quotient).
    { symmetry. apply Nat.div_unique with (r := head - 128); [exact low_bound|lia]. }
    assert (exact_mod : (head - 128 + 128 * quotient) mod 128 = head - 128).
    { symmetry. apply Nat.mod_unique with (q := quotient); [exact low_bound|lia]. }
    change (head :: tail = encode_varint_fuel (S fuel) (head - 128 + 128 * quotient) ++ rest).
    rewrite encode_varint_fuel_step, large, exact_div, exact_mod.
    replace (128 + (head - 128)) with head by lia.
    cbn [app]. now rewrite <- induction.
Qed.

Lemma decoded_varint_reconstructs_bytes :
  forall input value rest,
    decode_varint input = Some (value, rest) ->
    input = encode_varint value ++ rest.
Proof.
  intros. unfold encode_varint. eapply decoded_varint_reconstructs_fuel; [lia|eassumption].
Qed.

Theorem envelope_frame_exact_bytes :
  forall input body rest,
    decode_frame input = Some (body, rest) <-> input = encode_frame body ++ rest.
Proof.
  intros input body rest. split.
  - unfold decode_frame.
    destruct (decode_varint input) as [[size suffix]|] eqn:header; try discriminate.
    destruct (size <=? length suffix) eqn:bounded; try discriminate.
    intro same. inversion same; subst. apply Nat.leb_le in bounded.
    apply decoded_varint_reconstructs_bytes in header. rewrite header.
    unfold encode_frame. rewrite length_firstn, Nat.min_l by exact bounded.
    rewrite <- app_assoc, firstn_skipn. reflexivity.
  - intro same. subst. apply canonical_frame_round_trip.
Qed.

Definition strip_key (key : nat) (input : bytes) : option bytes :=
  match input with
  | head :: rest => if head =? key then Some rest else None
  | [] => None
  end.

Lemma strip_key_exact :
  forall key input rest, strip_key key input = Some rest <-> input = key :: rest.
Proof.
  intros key input rest. destruct input as [|head tail]; cbn [strip_key].
  - split; discriminate.
  - destruct (head =? key) eqn:same.
    + apply Nat.eqb_eq in same. subst. split; intro equal; inversion equal; reflexivity.
    + apply Nat.eqb_neq in same. split; intro equal; [discriminate|inversion equal; contradiction].
Qed.

Lemma strip_key_here : forall key rest, strip_key key (key :: rest) = Some rest.
Proof. intros. apply strip_key_exact. reflexivity. Qed.

Lemma strip_key_refines_canonical_field_cursor :
  forall key input rest,
    key < 128 ->
    (strip_key key input = Some rest <-> decode_varint input = Some (key, rest)).
Proof.
  intros key input rest bounded. assert (small : (key <? 128) = true) by now apply Nat.ltb_lt.
  split.
  - intro parsed. apply strip_key_exact in parsed. subst.
    now rewrite decode_varint_cons, small.
  - intro parsed. apply decoded_varint_reconstructs_bytes in parsed.
    unfold encode_varint in parsed. rewrite encode_varint_fuel_step, small in parsed.
    apply strip_key_exact. exact parsed.
Qed.

Definition snapshot_bytes (snapshot : bytes) : bytes := 74 :: encode_frame snapshot.
Definition remainder_bytes (remainder : option bytes) : bytes :=
  match remainder with None => [] | Some body => 42 :: encode_frame body end.
Definition connective_bytes (connective : bool) : bytes :=
  if connective then [32; 1] else [].
Definition locally_free_bytes (locally_free : option bytes) : bytes :=
  match locally_free with None => [] | Some body => 26 :: encode_frame body end.

Definition valid_remainder (remainder : option bytes) : Prop :=
  match remainder with
  | None => True
  | Some body => exists value, BinaryVar.parse_var body = Some value
  end.

Definition valid_locally_free (locally_free : option bytes) : Prop :=
  match locally_free with None => True | Some body => body <> [] end.

Definition snapshot_cursor (input : bytes) : option bytes :=
  match strip_key 74 input with
  | Some payload =>
      match decode_frame payload with Some (snapshot, []) => Some snapshot | _ => None end
  | None => None
  end.

Definition remainder_cursor (input : bytes) : option (option bytes * bytes) :=
  match strip_key 42 input with
  | Some payload =>
      match decode_frame payload with
      | Some (body, rest) =>
          match BinaryVar.parse_var body with
          | Some _ => option_map (fun snapshot => (Some body, snapshot)) (snapshot_cursor rest)
          | None => None
          end
      | None => None
      end
  | None => option_map (fun snapshot => (None, snapshot)) (snapshot_cursor input)
  end.

Definition connective_cursor (input : bytes) : option (bool * (option bytes * bytes)) :=
  match strip_key 32 input with
  | Some payload =>
      match decode_varint payload with
      | Some (1, rest) => option_map (fun parts => (true, parts)) (remainder_cursor rest)
      | _ => None
      end
  | None => option_map (fun parts => (false, parts)) (remainder_cursor input)
  end.

Record protobuf_envelope : Type := ProtobufEnvelope {
  protobuf_locally_free : option bytes;
  protobuf_connective : bool;
  protobuf_remainder : option bytes;
  protobuf_snapshot : bytes
}.

Definition protobuf_bytes (envelope_input : protobuf_envelope) : bytes :=
  locally_free_bytes (protobuf_locally_free envelope_input) ++
  connective_bytes (protobuf_connective envelope_input) ++
  remainder_bytes (protobuf_remainder envelope_input) ++ snapshot_bytes (protobuf_snapshot envelope_input).

Definition valid_protobuf (envelope_input : protobuf_envelope) : Prop :=
  valid_locally_free (protobuf_locally_free envelope_input) /\ valid_remainder (protobuf_remainder envelope_input).

Definition protobuf_cursor (input : bytes) : option protobuf_envelope :=
  match strip_key 26 input with
  | Some payload =>
      match decode_frame payload with
      | Some (head :: tail, rest) =>
          option_map
            (fun parts => ProtobufEnvelope (Some (head :: tail)) (fst parts)
              (fst (snd parts)) (snd (snd parts))) (connective_cursor rest)
      | _ => None
      end
  | None =>
      option_map
        (fun parts => ProtobufEnvelope None (fst parts) (fst (snd parts)) (snd (snd parts)))
        (connective_cursor input)
  end.

Lemma snapshot_cursor_exact :
  forall input snapshot,
    snapshot_cursor input = Some snapshot <-> input = snapshot_bytes snapshot.
Proof.
  intros input snapshot. split.
  - unfold snapshot_cursor.
    destruct (strip_key 74 input) as [payload|] eqn:key; try discriminate.
    destruct (decode_frame payload) as [[body rest]|] eqn:frame; try discriminate.
    destruct rest; try discriminate. intro same. inversion same; subst.
    apply strip_key_exact in key. apply envelope_frame_exact_bytes in frame.
    rewrite app_nil_r in frame. subst. reflexivity.
  - intro same. subst. unfold snapshot_cursor, snapshot_bytes. rewrite strip_key_here.
    pose proof (canonical_frame_round_trip snapshot []) as frame.
    rewrite app_nil_r in frame. now rewrite frame.
Qed.

Lemma remainder_cursor_sound :
  forall input remainder snapshot,
    remainder_cursor input = Some (remainder, snapshot) ->
    valid_remainder remainder /\
    input = remainder_bytes remainder ++ snapshot_bytes snapshot.
Proof.
  intros input remainder snapshot. unfold remainder_cursor.
  destruct (strip_key 42 input) as [payload|] eqn:key.
  - destruct (decode_frame payload) as [[body rest]|] eqn:frame; try discriminate.
    destruct (BinaryVar.parse_var body) as [value|] eqn:var; try discriminate.
    destruct (snapshot_cursor rest) as [observed|] eqn:terminal; cbn [option_map]; try discriminate.
    intro same. inversion same; subst.
    split; [now exists value|].
    apply strip_key_exact in key. apply envelope_frame_exact_bytes in frame.
    apply snapshot_cursor_exact in terminal. subst. reflexivity.
  - destruct (snapshot_cursor input) as [observed|] eqn:terminal; cbn [option_map]; try discriminate.
    intro same. inversion same; subst. split; [exact I|].
    now apply snapshot_cursor_exact in terminal.
Qed.

Lemma remainder_cursor_complete :
  forall remainder snapshot,
    valid_remainder remainder ->
    remainder_cursor (remainder_bytes remainder ++ snapshot_bytes snapshot) =
      Some (remainder, snapshot).
Proof.
  intros [body|] snapshot valid.
  - destruct valid as [value var]. unfold remainder_bytes, remainder_cursor.
    cbn [app]. rewrite strip_key_here, canonical_frame_round_trip, var.
    rewrite (proj2 (snapshot_cursor_exact _ _) eq_refl). reflexivity.
  - unfold remainder_bytes, remainder_cursor. cbn [app snapshot_bytes strip_key].
    rewrite (proj2 (snapshot_cursor_exact _ _) eq_refl). reflexivity.
Qed.

Lemma connective_cursor_sound :
  forall input connective remainder snapshot,
    connective_cursor input = Some (connective, (remainder, snapshot)) ->
    valid_remainder remainder /\
    input = connective_bytes connective ++ remainder_bytes remainder ++ snapshot_bytes snapshot.
Proof.
  intros input connective remainder snapshot. unfold connective_cursor.
  destruct (strip_key 32 input) as [payload|] eqn:key.
  - destruct (decode_varint payload) as [[value rest]|] eqn:scalar; try discriminate.
    destruct value as [|[|value]]; try discriminate.
    destruct (remainder_cursor rest) as [[observed body]|] eqn:child;
      cbn [option_map]; try discriminate.
    intro same. inversion same; subst.
    apply remainder_cursor_sound in child. destruct child as [valid exact_bytes].
    split; [exact valid|].
    apply strip_key_exact in key. apply decoded_varint_reconstructs_bytes in scalar.
    cbn [encode_varint encode_varint_fuel] in scalar. subst. reflexivity.
  - destruct (remainder_cursor input) as [[observed body]|] eqn:child;
      cbn [option_map]; try discriminate.
    intro same. inversion same; subst. now apply remainder_cursor_sound in child.
Qed.

Lemma connective_cursor_complete :
  forall connective remainder snapshot,
    valid_remainder remainder ->
    connective_cursor
      (connective_bytes connective ++ remainder_bytes remainder ++ snapshot_bytes snapshot) =
      Some (connective, (remainder, snapshot)).
Proof.
  intros connective remainder snapshot valid. destruct connective.
  - unfold connective_bytes, connective_cursor. cbn [app]. rewrite strip_key_here.
    rewrite decode_varint_cons. cbn.
    now rewrite remainder_cursor_complete by exact valid.
  - assert (absent : strip_key 32 (remainder_bytes remainder ++ snapshot_bytes snapshot) = None)
      by (destruct remainder; reflexivity).
    unfold connective_bytes, connective_cursor. cbn [app]. rewrite absent.
    now rewrite remainder_cursor_complete by exact valid.
Qed.

Lemma protobuf_cursor_sound :
  forall input envelope_input, protobuf_cursor input = Some envelope_input ->
    valid_protobuf envelope_input /\ input = protobuf_bytes envelope_input.
Proof.
  intros input envelope_input. unfold protobuf_cursor.
  destruct (strip_key 26 input) as [payload|] eqn:key.
  - destruct (decode_frame payload) as [[body rest]|] eqn:frame; try discriminate.
    destruct body as [|head tail]; try discriminate.
    destruct (connective_cursor rest) as [[connective [remainder snapshot]]|] eqn:child;
      cbn [option_map fst snd]; try discriminate.
    intro same. inversion same; subst.
    apply connective_cursor_sound in child. destruct child as [valid exact_bytes].
    split; [split; [discriminate|exact valid]|].
    apply strip_key_exact in key. apply envelope_frame_exact_bytes in frame. subst. reflexivity.
  - destruct (connective_cursor input) as [[connective [remainder snapshot]]|] eqn:child;
      cbn [option_map fst snd]; try discriminate.
    intro same. inversion same; subst.
    apply connective_cursor_sound in child. destruct child as [valid exact_bytes].
    split; [split; [exact I|exact valid]|exact exact_bytes].
Qed.

Lemma protobuf_cursor_complete :
  forall envelope_input, valid_protobuf envelope_input ->
    protobuf_cursor (protobuf_bytes envelope_input) = Some envelope_input.
Proof.
  intros [locally_free connective remainder snapshot] [valid_lf valid_rem].
  destruct locally_free as [body|].
  - destruct body as [|head tail]; [contradiction|].
    unfold protobuf_cursor, protobuf_bytes. cbn [protobuf_locally_free protobuf_connective protobuf_remainder protobuf_snapshot locally_free_bytes app].
    rewrite strip_key_here, canonical_frame_round_trip.
    rewrite connective_cursor_complete by exact valid_rem. reflexivity.
  - assert (absent : strip_key 26
        (connective_bytes connective ++ remainder_bytes remainder ++ snapshot_bytes snapshot) = None)
      by (destruct connective, remainder; reflexivity).
    unfold protobuf_cursor, protobuf_bytes.
    cbn [protobuf_locally_free protobuf_connective protobuf_remainder protobuf_snapshot locally_free_bytes app].
    rewrite absent, connective_cursor_complete by exact valid_rem. reflexivity.
Qed.

Theorem ordered_envelope_cursor_exact_grammar :
  forall input envelope_input,
    protobuf_cursor input = Some envelope_input <->
    valid_protobuf envelope_input /\ input = protobuf_bytes envelope_input.
Proof.
  intros input envelope_input. split; [apply protobuf_cursor_sound|].
  intros [valid ->]. now apply protobuf_cursor_complete.
Qed.

Record envelope : Type := Envelope {
  snapshot : bytes;
  connective_used : bool;
  locally_free_is_empty : bool;
  remainder : option bytes
}.

Definition observe (envelope_input : protobuf_envelope) : envelope :=
  Envelope (protobuf_snapshot envelope_input) (protobuf_connective envelope_input)
    (match protobuf_locally_free envelope_input with None => true | Some _ => false end)
    (protobuf_remainder envelope_input).

Definition envelope_cursor (input : bytes) : option envelope := option_map observe (protobuf_cursor input).

Theorem envelope_observation_exact :
  forall input observed,
    envelope_cursor input = Some observed <->
    exists envelope_input, valid_protobuf envelope_input /\
      input = protobuf_bytes envelope_input /\ observed = observe envelope_input.
Proof.
  intros input observed. unfold envelope_cursor. split.
  - destruct (protobuf_cursor input) as [envelope_input|] eqn:parsed; cbn [option_map]; try discriminate.
    intro same. inversion same; subst. apply protobuf_cursor_sound in parsed.
    exists envelope_input. tauto.
  - intros [envelope_input [valid [same observed_same]]]. subst.
    rewrite protobuf_cursor_complete by exact valid. reflexivity.
Qed.

Theorem successful_envelope_remainder_is_canonical :
  forall input observed body,
    envelope_cursor input = Some observed -> remainder observed = Some body ->
    exists value, BinaryVar.valid_var value /\ body = BinaryVar.encode_var value.
Proof.
  intros input observed body parsed present.
  apply envelope_observation_exact in parsed.
  destruct parsed as [envelope_input [[valid_lf valid_rem] [same ->]]].
  cbn [observe remainder] in present. rewrite present in valid_rem.
  destruct valid_rem as [value parsed]. exists value.
  now apply BinaryVar.canonical_var_exact_encoder_image in parsed.
Qed.

Lemma rejected_remainder_cursor :
  forall body suffix,
    BinaryVar.parse_var body = None ->
    remainder_cursor ((42 :: encode_frame body) ++ suffix) = None.
Proof.
  intros body suffix rejected. unfold remainder_cursor. cbn [app].
  now rewrite strip_key_here, canonical_frame_round_trip, rejected.
Qed.

Lemma rejected_connective_cursor :
  forall connective body suffix,
    BinaryVar.parse_var body = None ->
    connective_cursor
      (connective_bytes connective ++ (42 :: encode_frame body) ++ suffix) = None.
Proof.
  intros connective body suffix rejected. destruct connective;
    unfold connective_bytes, connective_cursor; cbn [app strip_key].
  - change (option_map (fun parts => (true, parts))
      (remainder_cursor ((42 :: encode_frame body) ++ suffix)) = None).
    now rewrite rejected_remainder_cursor by exact rejected.
  - change (option_map (fun parts => (false, parts))
      (remainder_cursor ((42 :: encode_frame body) ++ suffix)) = None).
    now rewrite rejected_remainder_cursor by exact rejected.
Qed.

Theorem later_bad_remainder_rejects_independently :
  forall locally_free connective body suffix,
    valid_locally_free locally_free -> BinaryVar.parse_var body = None ->
    envelope_cursor
      (locally_free_bytes locally_free ++ connective_bytes connective ++
        (42 :: encode_frame body) ++ suffix) = None.
Proof.
  intros locally_free connective body suffix valid_lf rejected.
  pose proof (@rejected_connective_cursor connective body suffix rejected) as child.
  cbn [app] in child.
  destruct locally_free as [lf|]; unfold envelope_cursor, protobuf_cursor, locally_free_bytes.
  - destruct lf as [|head tail]; [contradiction|].
    cbn [app]. rewrite strip_key_here, canonical_frame_round_trip.
    now rewrite child.
  - assert (absent : strip_key 26
    (connective_bytes connective ++ (42 :: encode_frame body) ++ suffix) = None)
      by (destruct connective; reflexivity).
    cbn [app] in absent |- *. rewrite absent.
    now rewrite child.
Qed.

Theorem envelope_remainder_presence_is_distinct :
  envelope_cursor [74; 0] = Some (Envelope [] false true None) /\
  envelope_cursor [42; 0; 74; 0] = Some (Envelope [] false true (Some [])) /\
  Envelope [] false true None <> Envelope [] false true (Some []).
Proof. vm_compute. intuition discriminate. Qed.

Theorem envelope_positive_metadata_controls :
  envelope_cursor [26; 1; 7; 32; 1; 42; 2; 8; 0; 74; 1; 255] =
    Some (Envelope [255] true false (Some [8; 0])) /\
  envelope_cursor [42; 2; 16; 1; 74; 0] =
    Some (Envelope [] false true (Some [16; 1])) /\
  envelope_cursor [42; 2; 26; 0; 74; 0] =
    Some (Envelope [] false true (Some [26; 0])).
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem envelope_order_and_duplicate_controls :
  envelope_cursor [32; 1; 26; 1; 7; 74; 0] = None /\
  envelope_cursor [26; 1; 7; 26; 1; 8; 74; 0] = None /\
  envelope_cursor [32; 1; 32; 1; 74; 0] = None /\
  envelope_cursor [42; 0; 42; 0; 74; 0] = None /\
  envelope_cursor [74; 0; 74; 0] = None /\
  envelope_cursor [74; 0; 32; 1] = None.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem envelope_framing_and_scalar_refusal_controls :
  envelope_cursor [] = None /\
  envelope_cursor [26; 0; 74; 0] = None /\
  envelope_cursor [32; 0; 74; 0] = None /\
  envelope_cursor [32; 129; 0; 74; 0] = None /\
  envelope_cursor [24; 1; 74; 0] = None /\
  envelope_cursor [8; 0; 74; 0] = None /\
  envelope_cursor [202; 0; 0] = None /\
  envelope_cursor [74; 128; 0] = None /\
  envelope_cursor [74; 2; 0] = None /\
  envelope_cursor [26; 1; 7; 32; 1; 42; 1; 8; 74; 0] = None.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem snapshot_bytes_remain_opaque :
  forall body,
    envelope_cursor (snapshot_bytes body) = Some (Envelope body false true None).
Proof.
  intro body. change (envelope_cursor (protobuf_bytes (ProtobufEnvelope None false None body)) =
    Some (Envelope body false true None)).
  unfold envelope_cursor. rewrite protobuf_cursor_complete; [reflexivity|split; exact I].
Qed.

Print Assumptions envelope_frame_exact_bytes.
Print Assumptions ordered_envelope_cursor_exact_grammar.
Print Assumptions envelope_observation_exact.
Print Assumptions successful_envelope_remainder_is_canonical.
Print Assumptions later_bad_remainder_rejects_independently.
Print Assumptions envelope_remainder_presence_is_distinct.
Print Assumptions envelope_positive_metadata_controls.
Print Assumptions envelope_order_and_duplicate_controls.
Print Assumptions envelope_framing_and_scalar_refusal_controls.
Print Assumptions snapshot_bytes_remain_opaque.

End Envelope.
