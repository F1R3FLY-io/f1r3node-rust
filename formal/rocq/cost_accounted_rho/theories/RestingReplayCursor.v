(* A protocol-7 replay expectation is a global ordered cursor, not a second
   multiset binding for an arriving Produce or Consume.  Rigging installs the
   expected sequence without retiring any datum.  Each replay transition
   checks the next event before its store mutation, and a refusal leaves the
   cursor, retirement count, and host-work charge unchanged.

   This is the admission law for the RSpace replay implementation; it does
   not claim that the existing ReplayRSpace replay_data multiset is already
   an ordered cursor or that a validator-side lookahead trigger exists. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Inductive event_kind := Produce | Consume | Comm | RestingComm.

(* The natural identity stands for the authenticated, ordered event payload:
   a matching event kind alone is never sufficient to advance replay. *)
Definition event := (event_kind * nat)%type.

Record replay_cursor := {
  expected : list event;
  accepted : list event;
  pending : list event;
  retirements : nat;
  charged_commits : nat
}.

Definition is_resting (entry : event) : nat :=
  match fst entry with RestingComm => 1 | _ => 0 end.

Definition event_eq_dec : forall (left right : event),
  {left = right} + {left <> right}.
Proof. decide equality; try apply Nat.eq_dec; decide equality. Defined.

Definition rig (events : list event) : replay_cursor :=
  {| expected := events;
     accepted := [];
     pending := events;
     retirements := 0;
     charged_commits := 0 |}.

Definition advance (state : replay_cursor) (entry : event) : option replay_cursor :=
  match pending state with
  | [] => None
  | next :: tail =>
      if event_eq_dec next entry then
        Some
          {| expected := expected state;
             accepted := accepted state ++ [entry];
             pending := tail;
             retirements := retirements state + is_resting entry;
             charged_commits := charged_commits state + is_resting entry |}
      else None
  end.

Definition ordered (state : replay_cursor) : Prop :=
  accepted state ++ pending state = expected state.

Definition charge_equal (state : replay_cursor) : Prop :=
  retirements state = charged_commits state.

Theorem rig_does_not_retire : forall events,
  ordered (rig events) /\ charge_equal (rig events) /\
  retirements (rig events) = 0.
Proof. intros; unfold ordered, charge_equal, rig; simpl; auto. Qed.

Theorem advance_preserves_order : forall state event next,
  ordered state ->
  advance state event = Some next ->
  ordered next.
Proof.
  intros state event next ordered_state advanced.
  unfold advance in advanced.
  destruct (pending state) as [|head tail] eqn:pending_state; [discriminate|].
  destruct (event_eq_dec head event) as [same|different];
    [subst head|discriminate].
  inversion advanced; subst next; clear advanced.
  unfold ordered in *; simpl in *.
  rewrite <- app_assoc. simpl.
  rewrite pending_state in ordered_state. exact ordered_state.
Qed.

Theorem advance_preserves_charge : forall state event next,
  charge_equal state ->
  advance state event = Some next ->
  charge_equal next.
Proof.
  intros state event next equal_state advanced.
  unfold advance in advanced.
  destruct (pending state) as [|head tail]; [discriminate|].
  destruct (event_eq_dec head event); [|discriminate].
  inversion advanced; subst next; clear advanced.
  unfold charge_equal in *; simpl in *; lia.
Qed.

Theorem mismatch_refuses_without_mutation : forall state event,
  (pending state = [] \/
   exists head tail, pending state = head :: tail /\ head <> event) ->
  advance state event = None.
Proof.
  intros state event [empty|different].
  - unfold advance. rewrite empty. reflexivity.
  - destruct different as [head [tail [pending_state mismatch]]].
    unfold advance. rewrite pending_state.
    destruct (event_eq_dec head event); [contradiction|reflexivity].
Qed.

Print Assumptions rig_does_not_retire.
Print Assumptions advance_preserves_order.
Print Assumptions advance_preserves_charge.
Print Assumptions mismatch_refuses_without_mutation.
