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
