From Stdlib Require Import List Arith Bool.
Import ListNotations.

Record TailState := {
  term_id : nat;
  environment : list nat;
  randomness : nat;
  authority : nat;
  causal_path : list nat;
  metering_path : list nat;
  residue_seal : bool;
}.

Inductive TailEvent :=
| ChargeStep
| ChargeBytes (amount : nat)
| ObserveBundle (body : nat)
| OtherEvent (kind : nat).

Inductive TailOutcome :=
| Succeeded
| Failed (error : nat).

Inductive TailDecision :=
| Halt (outcome : TailOutcome)
| Advance (next : TailState).

Definition TailTransition := TailState -> list TailEvent * TailDecision.

Fixpoint recursive_tail
  (transition : TailTransition) (fuel : nat) (state : TailState)
  : option (list TailEvent * TailOutcome) :=
  match fuel with
  | O => None
  | S remaining =>
      let '(events, decision) := transition state in
      match decision with
      | Halt outcome => Some (events, outcome)
      | Advance next =>
          match recursive_tail transition remaining next with
          | None => None
          | Some (later, outcome) => Some (events ++ later, outcome)
          end
      end
  end.

Fixpoint machine_tail
  (transition : TailTransition) (fuel : nat)
  (state : TailState) (events : list TailEvent)
  : option (list TailEvent * TailOutcome) :=
  match fuel with
  | O => None
  | S remaining =>
      let '(now, decision) := transition state in
      match decision with
      | Halt outcome => Some (events ++ now, outcome)
      | Advance next =>
          machine_tail transition remaining next (events ++ now)
      end
  end.

Theorem tail_machine_preserves_trace_and_outcome :
  forall transition fuel state prefix,
    machine_tail transition fuel state prefix =
    option_map
      (fun result => (prefix ++ fst result, snd result))
      (recursive_tail transition fuel state).
Proof.
  intros transition fuel.
  induction fuel as [| remaining IH]; intros state prefix; simpl.
  - reflexivity.
  - destruct (transition state) as [now decision] eqn:Htransition.
    destruct decision as [outcome | next]; simpl.
    + reflexivity.
    + rewrite IH.
      destruct (recursive_tail transition remaining next)
        as [[later outcome] |] eqn:Hlater; simpl.
      * now rewrite app_assoc.
      * reflexivity.
Qed.

Theorem tail_machine_stops_on_failure :
  forall transition fuel state prefix now error,
    transition state = (now, Halt (Failed error)) ->
    machine_tail transition (S fuel) state prefix =
      Some (prefix ++ now, Failed error).
Proof.
  intros transition fuel state prefix now error Htransition.
  simpl. now rewrite Htransition.
Qed.

Theorem tail_machine_advances_once :
  forall transition fuel state prefix now next,
    transition state = (now, Advance next) ->
    machine_tail transition (S fuel) state prefix =
      machine_tail transition fuel next (prefix ++ now).
Proof.
  intros transition fuel state prefix now next Htransition.
  simpl. now rewrite Htransition.
Qed.

Definition bundle_next (state : TailState) (body : nat) : TailState :=
  {| term_id := body;
     environment := environment state;
     randomness := randomness state;
     authority := authority state;
     causal_path := causal_path state;
     metering_path := metering_path state;
     residue_seal := residue_seal state |}.

Definition bundle_transition
  (bytes body : nat) (state : TailState)
  : list TailEvent * TailDecision :=
  ([ChargeStep; ChargeBytes bytes; ObserveBundle body],
   Advance (bundle_next state body)).

Theorem bundle_step_preserves_authority_and_context :
  forall state body,
    environment (bundle_next state body) = environment state /\
    randomness (bundle_next state body) = randomness state /\
    authority (bundle_next state body) = authority state /\
    causal_path (bundle_next state body) = causal_path state /\
    metering_path (bundle_next state body) = metering_path state /\
    residue_seal (bundle_next state body) = residue_seal state.
Proof.
  intros state body.
  repeat split; reflexivity.
Qed.

Theorem bundle_step_orders_charge_before_observation :
  forall bytes body state,
    fst (bundle_transition bytes body state) =
      [ChargeStep; ChargeBytes bytes; ObserveBundle body].
Proof.
  reflexivity.
Qed.

Inductive TailForm :=
| BundleTail
| IfTail
| NewTail (extended_environment : list nat)
| MatchTail (case_environment : list nat)
| VariableTail
| MethodTail.

Definition tail_next
  (form : TailForm) (state : TailState) (body : nat) : TailState :=
  {| term_id := body;
     environment :=
       match form with
       | NewTail next_environment | MatchTail next_environment => next_environment
       | _ => environment state
       end;
     randomness := randomness state;
     authority := authority state;
     causal_path := causal_path state;
     metering_path := metering_path state;
     residue_seal :=
       match form with
       | VariableTail | MethodTail => false
       | _ => residue_seal state
       end |}.

Theorem every_tail_form_preserves_runtime_context :
  forall form state body,
    randomness (tail_next form state body) = randomness state /\
    authority (tail_next form state body) = authority state /\
    causal_path (tail_next form state body) = causal_path state /\
    metering_path (tail_next form state body) = metering_path state.
Proof.
  intros form state body.
  repeat split; reflexivity.
Qed.

Theorem scoped_tail_forms_use_the_selected_environment :
  forall state body next_environment,
    environment (tail_next (NewTail next_environment) state body) =
      next_environment /\
    environment (tail_next (MatchTail next_environment) state body) =
      next_environment.
Proof.
  intros state body next_environment.
  split; reflexivity.
Qed.

Theorem reentry_tail_forms_clear_only_the_seal :
  forall state body,
    residue_seal (tail_next VariableTail state body) = false /\
    residue_seal (tail_next MethodTail state body) = false /\
    environment (tail_next VariableTail state body) = environment state /\
    environment (tail_next MethodTail state body) = environment state.
Proof.
  intros state body.
  repeat split; reflexivity.
Qed.

Theorem bundle_tail_agrees_with_bundle_step :
  forall state body,
    tail_next BundleTail state body = bundle_next state body.
Proof.
  intros state body.
  reflexivity.
Qed.
