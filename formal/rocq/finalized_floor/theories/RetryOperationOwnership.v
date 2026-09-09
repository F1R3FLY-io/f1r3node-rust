From Coq Require Import Arith.PeanoNat Bool.Bool Lia.

Section RetryOwnership.
Context {Owner Operation : Type}.
Variable owner_eq : forall x y : Owner, {x = y} + {x <> y}.
Variable operation_eq : forall x y : Operation, {x = y} + {x <> y}.
Variable CounterLimit : nat.

Definition bump (n : nat) : nat := Nat.min CounterLimit (S n).
Definition increment (counter : Owner -> nat) (owner : Owner) : Owner -> nat :=
  fun other => if owner_eq other owner then bump (counter other) else counter other.

Record State := {
  live : Owner -> bool;
  counters : Owner -> nat;
  peer_counters : Owner -> nat;
  tokens : Operation -> option (Owner * bool)
}.

Definition consume (ts : Operation -> option (Owner * bool)) (op : Operation) :=
  fun other => if operation_eq other op then None else ts other.

Definition complete (s : State) (op : Operation) (observed : bool) : State :=
  match tokens s op with
  | None => s
  | Some (origin, peer) =>
      let charge := observed && live s origin in
      {| live := live s;
         counters := if charge then increment (counters s) origin else counters s;
         peer_counters := if charge && peer
                          then increment (peer_counters s) origin else peer_counters s;
         tokens := consume (tokens s) op |}
  end.

Lemma consumed_token_absent ts op : consume ts op op = None.
Proof. unfold consume. destruct (operation_eq op op); congruence. Qed.

Lemma consume_preserves_other_tokens ts op other :
  other <> op -> consume ts op other = ts other.
Proof. intros H. unfold consume. destruct (operation_eq other op); congruence. Qed.

Theorem completion_consumes_token s op observed :
  tokens (complete s op observed) op = None.
Proof.
  unfold complete. destruct (tokens s op) as [[origin peer]|] eqn:H.
  - simpl. apply consumed_token_absent.
  - exact H.
Qed.

Theorem duplicate_completion_is_identity s op first second :
  complete (complete s op first) op second = complete s op first.
Proof.
  unfold complete at 1. rewrite completion_consumes_token. reflexivity.
Qed.

Theorem cancellation_preserves_counters s op :
  counters (complete s op false) = counters s /\
  peer_counters (complete s op false) = peer_counters s.
Proof.
  unfold complete. destruct (tokens s op) as [[origin peer]|]; split; reflexivity.
Qed.

Theorem completion_preserves_liveness s op observed :
  live (complete s op observed) = live s.
Proof. unfold complete. destruct (tokens s op) as [[origin peer]|]; reflexivity. Qed.

Theorem terminal_completion_preserves_counters s op origin peer observed :
  tokens s op = Some (origin, peer) -> live s origin = false ->
  counters (complete s op observed) = counters s /\
  peer_counters (complete s op observed) = peer_counters s.
Proof.
  intros Htoken Hlive. unfold complete. rewrite Htoken, Hlive, andb_false_r.
  split; reflexivity.
Qed.

Lemma increment_preserves_other_owner c origin other :
  other <> origin -> increment c origin other = c other.
Proof. intros H. unfold increment. destruct (owner_eq other origin); congruence. Qed.

Theorem completion_cannot_charge_replacement s op origin peer other observed :
  tokens s op = Some (origin, peer) -> other <> origin ->
  counters (complete s op observed) other = counters s other /\
  peer_counters (complete s op observed) other = peer_counters s other.
Proof.
  intros Htoken Hother. unfold complete. rewrite Htoken. simpl.
  destruct (observed && live s origin), peer; simpl;
    split; try reflexivity; apply increment_preserves_other_owner; exact Hother.
Qed.

Theorem successful_completion_charges_origin s op origin peer :
  tokens s op = Some (origin, peer) -> live s origin = true ->
  counters (complete s op true) origin = bump (counters s origin).
Proof.
  intros Htoken Hlive. unfold complete. rewrite Htoken, Hlive. simpl.
  unfold increment. destruct (owner_eq origin origin); congruence.
Qed.

Theorem successful_peer_completion_charges_origin s op origin :
  tokens s op = Some (origin, true) -> live s origin = true ->
  peer_counters (complete s op true) origin = bump (peer_counters s origin).
Proof.
  intros Htoken Hlive. unfold complete. rewrite Htoken, Hlive. simpl.
  unfold increment. destruct (owner_eq origin origin); congruence.
Qed.

Theorem counter_bound n : bump n <= CounterLimit.
Proof. unfold bump. apply Nat.le_min_l. Qed.

Theorem counter_monotonic n : n <= CounterLimit -> n <= bump n.
Proof. intros H. unfold bump. apply Nat.min_glb; lia. Qed.

Theorem reservation_preserves_allowance completed reserved allowance :
  completed + reserved < allowance -> completed + S reserved <= allowance.
Proof. lia. Qed.

Theorem success_preserves_reserved_allowance completed reserved allowance :
  completed + S reserved <= allowance -> S completed + reserved <= allowance.
Proof. lia. Qed.

Theorem cancellation_preserves_completed_count completed reserved allowance :
  completed + S reserved <= allowance -> completed + reserved <= allowance.
Proof. lia. Qed.


Inductive ActionOutcome := ReturnedOk | ReturnedError | Cancelled.

Definition finish_action (s : State) (op : Operation) (outcome : ActionOutcome) :=
  complete s op (match outcome with Cancelled => false | _ => true end).

Theorem returned_error_and_success_charge_identically s op :
  finish_action s op ReturnedError = finish_action s op ReturnedOk.
Proof. reflexivity. Qed.

Theorem returned_error_charges_origin s op origin peer :
  tokens s op = Some (origin, peer) -> live s origin = true ->
  counters (finish_action s op ReturnedError) origin = bump (counters s origin).
Proof. apply successful_completion_charges_origin. Qed.

Theorem cancelled_action_preserves_counters s op :
  counters (finish_action s op Cancelled) = counters s /\
  peer_counters (finish_action s op Cancelled) = peer_counters s.
Proof. apply cancellation_preserves_counters. Qed.

End RetryOwnership.

Print Assumptions completion_consumes_token.
Print Assumptions duplicate_completion_is_identity.
Print Assumptions cancellation_preserves_counters.
Print Assumptions completion_preserves_liveness.
Print Assumptions terminal_completion_preserves_counters.
Print Assumptions completion_cannot_charge_replacement.
Print Assumptions successful_completion_charges_origin.
Print Assumptions successful_peer_completion_charges_origin.
Print Assumptions counter_bound.
Print Assumptions counter_monotonic.
Print Assumptions reservation_preserves_allowance.
Print Assumptions success_preserves_reserved_allowance.
Print Assumptions cancellation_preserves_completed_count.

Print Assumptions returned_error_and_success_charge_identically.
Print Assumptions returned_error_charges_origin.
Print Assumptions cancelled_action_preserves_counters.
